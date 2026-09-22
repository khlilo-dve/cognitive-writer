#!/usr/bin/env python3
"""Verify Transfer v2 artifacts and immutable inputs offline; performs no model requests."""
import argparse
import hashlib
import json
from pathlib import Path

DIMENSIONS = ("style_fidelity", "source_independence", "factual_compliance", "naturalness", "task_completion")
METHODS = ("single_stage", "two_stage", "direct_free")
GROUPS = ("historical", "strict", "creative")


def read(path):
    return json.loads(path.read_text())


def raw_json(text):
    text = text.strip()
    if text.startswith("```json") and text.endswith("```"):
        text = text[7:-3].strip()
    value = json.loads(text)
    for key in ("task_issues", "uncertainties"):
        if isinstance(value.get(key), str): value[key] = [value[key]] if value[key].strip() else []
    return value


def length(body):
    return sum(not c.isspace() for c in body)


def reference(source):
    return source.split("\n", 1)[1].strip() if source.startswith("# 样文 ") else source.strip()


def verify(run, current=False):
    inputs, summary = read(run / "inputs.json"), read(run / "summary.json")
    assert summary["status"] == "complete" and summary["rubric"] == "Transfer-v2"
    assert summary["model"] == inputs["model"] and not summary["automatic_adoption"]
    jobs = {j["id"]: j for j in inputs["jobs"]}
    rows = summary["rows"]
    assert len(jobs) == len(rows) == 45
    assert {r["id"] for r in rows} == set(jobs)
    assert {(r["group"], r["variant"], r["sample"]) for r in rows} == {
        (g, m, i) for g in GROUPS for m in METHODS for i in range(1, 6)}
    requests = 0
    repo = Path(__file__).resolve().parents[1]

    def judge_checks(item, row, body, calls):
        for attempt in range(calls):
            cp = read(run / f"{item['id']}_judge_{attempt}.json")
            req = cp["request"]
            assert set(req) == {"schema", "endpoint_fingerprint", "model", "system", "user"}
            assert req["model"] == inputs["model"] and req["system"] == inputs["judge_prompt"]
            user = json.loads(req["user"])
            if attempt:
                user = user["original_input"]
            assert set(user) == {"reference", "task", "fact_mode", "mechanical_length", "article"}
            assert user["reference"] == reference(item["source"])
            assert user["task"] == item["topic"] and user["fact_mode"] == item["mode"]
            assert user["article"] == body and user["mechanical_length"]["characters"] == length(body)
        judgment = raw_json(cp["response"])
        assert judgment == row["judgment"]
        assert set(judgment["scores"]) == set(DIMENSIONS)
        assert all(1 <= value <= 5 for value in judgment["scores"].values())
        assert "overall_score" not in judgment
        for v in judgment["source_leakage"]:
            assert v["article_quote"] and v["article_quote"] in body
            assert v["source_quote"] and v["source_quote"] in reference(item["source"])
        for v in judgment["fact_violations"]:
            assert v["article_quote"] and v["article_quote"] in body
        for score, evidence in (("source_independence", "source_leakage"), ("factual_compliance", "fact_violations"), ("task_completion", "task_issues")):
            assert (judgment["scores"][score] == 5) == (not judgment[evidence])
        return calls

    for row in rows:
        job = jobs[row["id"]]
        for key in ("group", "sample", "variant"):
            assert job[key] == row[key]
        assert read(run / f"{row['id']}_result.json") == row
        body = (run / row["body_artifact"]).read_text()
        assert row["length"]["characters"] == length(body)
        assert row["length"]["within_range"] == (600 <= length(body) <= 900)
        origin = repo / job["provenance"]["run"]
        assert (origin / job["provenance"]["spec"]).read_text() == job["spec"]
        if job["existing_body"] is not None:
            assert body == job["existing_body"] == (origin / job["provenance"]["body"]).read_text()
            assert row["generation_calls"] == 0
        else:
            outline = read(run / f"{job['id']}_outline.json")
            final = read(run / f"{job['id']}_body.json")
            assert outline["request"]["system"] == inputs["outline_template"].replace("{style}", job["spec"])
            assert outline["request"]["user"] == job["topic"]
            assert final["request"]["system"] == inputs["body_template"].replace("{style}", job["spec"])
            assert final["request"]["user"] == inputs["render_template"].format(outline=outline["response"], idea=job["topic"])
            assert final["response"] == body and row["generation_calls"] == 2
            assert outline["request"]["model"] == final["request"]["model"] == inputs["model"]
            requests += 2
        assert 1 <= row["judge_calls"] <= 2
        requests += judge_checks(job, row, body, row["judge_calls"])
    controls = {c["id"]: c for c in inputs["controls"]}
    assert len(controls) == len(summary["controls"]) == 4
    assert {c["id"] for c in summary["controls"]} == set(controls)
    for result in summary["controls"]:
        control = controls[result["id"]]
        requests += judge_checks(control, result, control["body"], result["judge_calls"])
        scores = result["judgment"]["scores"]
        expected = scores["source_independence"] <= 3 if control["id"].startswith("leak_") else (
            scores["source_independence"] == 5 and (
                scores["factual_compliance"] <= 3 if control["mode"] == "strict" else scores["factual_compliance"] == 5))
        assert result["passed"] == expected
    assert summary["controls_passed"] == all(c["passed"] for c in summary["controls"])
    for kind in ("extra", "leak"):
        by_id = {c["id"]: c["judgment"]["scores"] for c in summary["controls"]}
        expected = {d: by_id[kind + "_creative"][d] - by_id[kind + "_strict"][d] for d in DIMENSIONS}
        drift = summary["control_mode_drift"][kind]
        assert drift["creative_minus_strict"] == expected
        assert drift["non_fact_drift_observed"] == any(abs(expected[d]) > .05 for d in DIMENSIONS if d != "factual_compliance")
    for group in GROUPS:
        for method in METHODS:
            selected = [r for r in rows if r["group"] == group and r["variant"] == method]
            aggregate = summary["aggregates"][group][method]
            for dim in DIMENSIONS:
                mean = sum(r["judgment"]["scores"][dim] for r in selected) / 5
                assert abs(aggregate["means"][dim] - mean) < 1e-9
            assert aggregate["length_passes"] == sum(r["length"]["within_range"] for r in selected)
            if method != "single_stage":
                legacy = {r["sample"]: r for r in rows if r["group"] == group and r["variant"] == "single_stage"}
                for dim in DIMENSIONS:
                    delta = [r["judgment"]["scores"][dim] - legacy[r["sample"]]["judgment"]["scores"][dim] for r in sorted(selected, key=lambda r: r["sample"])]
                    paired = aggregate["paired_against_legacy"][dim]
                    assert paired["deltas_by_sample"] == delta
                    assert paired["wins"] == sum(d > .05 for d in delta)
                    assert paired["losses"] == sum(d < -.05 for d in delta)
                    assert paired["ties"] == sum(abs(d) <= .05 for d in delta)
    provenance = run / "reused-checkpoints.json"
    if provenance.exists():
        for item in read(provenance)["files"]:
            assert hashlib.sha256((run / item["file"]).read_bytes()).hexdigest() == item["sha256"]
    if current:
        assert (repo / "prompts/eval_judge_transfer_v2.md").read_text() == inputs["judge_prompt"]
        for path, content in inputs["extraction_prompts_unchanged"].items():
            assert (repo / path).read_text() == content
    return {"status": "passed", "rows": len(rows), "control_cases": len(controls),
            "judge_controls_passed": summary["controls_passed"], "checkpoints": requests,
            "current_prompts_verified": current}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--current", action="store_true")
    args = parser.parse_args()
    print(json.dumps(verify(args.run, args.current), ensure_ascii=False))
