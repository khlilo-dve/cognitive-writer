#!/usr/bin/env python3
"""Offline evidence and predeclared gate checks for a free-form learner release run."""
import argparse
import hashlib
import json
from pathlib import Path

DIMS = ["style_fidelity", "source_independence", "factual_compliance", "naturalness", "task_completion"]
METHODS = ["single_stage", "direct_free"]
MODES = ["strict", "creative"]


def read(path):
    return json.loads(path.read_text())


def parse(text):
    text = text.strip()
    if text.startswith("```json") and text.endswith("```"):
        text = text[7:-3].strip()
    value = json.loads(text)
    for key in ("task_issues", "uncertainties"):
        if isinstance(value.get(key), str): value[key] = [value[key]] if value[key].strip() else []
    return value


def count(text):
    return sum(not c.isspace() for c in text)


def verify(root, current=False):
    repo = Path(__file__).resolve().parents[1]
    inputs, summary = read(root / "inputs.json"), read(root / "summary.json")
    assert summary["status"] == "complete" and summary["model"] == inputs["model"]
    specs, rows = summary["specs"], summary["rows"]
    assert len(specs) == 10 and {(s["sample"], s["variant"]) for s in specs} == {(i,m) for i in range(1,6) for m in METHODS}
    assert len(rows) == 20 and {(r["sample"], r["variant"], r["group"]) for r in rows} == {(i,m,g) for i in range(1,6) for m in METHODS for g in MODES}
    jobs = {j["id"]:j for j in read(root / "jobs.json")}
    assert len(jobs) == 20 and set(jobs) == {r["id"] for r in rows}
    requests = 0
    def checkpoint(name):
        nonlocal requests
        cp = read(root / (name + ".json")); requests += 1
        assert set(cp["request"]) == {"schema", "endpoint_fingerprint", "model", "system", "user"}
        assert cp["request"]["model"] == inputs["model"]
        return cp
    for s in specs:
        i,m = s["sample"],s["variant"]
        assert (root / f"spec_{m}_{i:02}.md").read_text() == s["text"]
        assert s["judgment"]["characters_without_whitespace"] == count(s["text"])
        if inputs["phase"] == "development":
            records = sorted(root.glob(f"learn_{m}_{i:02}_*.json"))
            assert len(records) == s["extraction_calls"]
            for f in records:
                cp = checkpoint(f.stem)
                assert cp["request"]["system"] == inputs["learn_prompt" if m == "direct_free" else "legacy_prompt"]
            assert records and read(records[0])["request"]["user"] == inputs["sources"][i-1]
            last_raw = read(records[-1])["response"]
            extracted = json.loads(last_raw)["spec"].strip() if m == "direct_free" else last_raw.strip()
            learned = read(root / f"learn_{m}_{i:02}.json")
            assert learned["spec"] == s["text"] == extracted
            cp = checkpoint(f"spec_{m}_{i:02}_judge")
            assert cp["request"]["system"] == inputs["spec_judge"] and s["text"] in cp["request"]["user"]
            raw = cp["response"]; raw = json.loads(raw[raw.index("{"):raw.rindex("}")+1])
            for dim in ["coverage","actionability","distinctiveness","cleanliness"]:
                assert raw["scores"][dim] == s["judgment"]["scores"][dim]
    if inputs["phase"] == "confirmation":
        original = read(repo / "eval/runs" / inputs["source_run"] / "summary.json")
        assert original["gate"]["passed"] and original["specs"] == specs == inputs["confirmed_specs"]
    for r in rows:
        job = jobs[r["id"]]
        s = next(s for s in specs if s["sample"] == r["sample"] and s["variant"] == r["variant"])
        assert job["spec"] == s["text"]
        assert job["source"] == inputs["sources"][r["sample"]-1]
        assert job["topic"] == inputs["all_preregistered_tasks"][inputs["phase"]][r["group"]]
        body = (root / r["body_artifact"]).read_text()
        assert r["length"]["characters"] == count(body)
        assert r["length"]["within_range"] == (600 <= count(body) <= 900)
        outline = checkpoint(r["id"] + "_outline")
        final = checkpoint(r["id"] + "_body")
        assert final["response"] == body
        assert outline["request"]["system"] == inputs["outline_template"].replace("{style}",s["text"])
        assert outline["request"]["user"] == job["topic"]
        assert final["request"]["system"] == inputs["body_template"].replace("{style}",s["text"])
        assert final["request"]["user"] == inputs["render_template"].format(outline=outline["response"],idea=job["topic"])
        for attempt in range(r["judge_calls"]):
            cp = checkpoint(r["id"] + f"_judge_{attempt}")
            assert cp["request"]["system"] == inputs["transfer_judge"]
            user = json.loads(cp["request"]["user"])
            if attempt: user = user["original_input"]
            assert set(user) == {"reference","task","fact_mode","mechanical_length","article"}
            ref = job["source"].split("\n",1)[1].strip() if job["source"].startswith("# 样文 ") else job["source"].strip()
            assert user["reference"] == ref and user["task"] == job["topic"]
            assert user["article"] == body and user["fact_mode"] == r["group"]
        assert parse(cp["response"]) == r["judgment"]
        for item in r["judgment"]["source_leakage"]:
            assert item["article_quote"] in body and item["source_quote"] in ref
        for item in r["judgment"]["fact_violations"]:
            assert item["article_quote"] in body
    candidate = [s for s in specs if s["variant"] == "direct_free"]
    passed = all(400 <= count(s["text"]) <= 600 and s["judgment"]["scores"]["cleanliness"] >= 4 for s in candidate)
    passed &= all(sum(s["judgment"]["scores"][dim] for s in candidate)/5 >= 4 for dim in ["actionability","distinctiveness"])
    for mode in MODES:
        minima = [4,4.8,4.4 if mode == "strict" else 4.8,3.8,4.8]
        for method in METHODS:
            rr = [r for r in rows if r["group"] == mode and r["variant"] == method]
            means = {k:sum(r["judgment"]["scores"][k] for r in rr)/5 for k in DIMS}
            actual = summary["gate"]["by_mode"][mode][method]
            assert means == actual["means"] and sum(r["length"]["within_range"] for r in rr) == actual["length_passes"]
            if method == "direct_free":
                passed &= all(means[k]+1e-9 >= threshold for k,threshold in zip(DIMS,minima))
                passed &= all(r["judgment"]["scores"][k] > 2 for r in rr for k in ["style_fidelity","source_independence","factual_compliance","task_completion"])
                passed &= actual["length_passes"] >= 4
        paired = summary["gate"]["by_mode"][mode].get("paired_counts")
        if paired is not None:
            for dim in DIMS:
                differences = [next(r for r in rows if r["group"] == mode and r["variant"] == "direct_free" and r["sample"] == i)["judgment"]["scores"][dim] - next(r for r in rows if r["group"] == mode and r["variant"] == "single_stage" and r["sample"] == i)["judgment"]["scores"][dim] for i in range(1,6)]
                assert paired[dim] == {"wins":sum(d>0 for d in differences), "ties":sum(d==0 for d in differences), "losses":sum(d<0 for d in differences)}
    assert bool(passed) == summary["gate"]["passed"]
    if current:
        for key,path in [("learn_prompt","prompts/learn_style_direct.md"),("spec_judge","prompts/eval_judge_learn.md"),("transfer_judge","prompts/eval_judge_transfer_v2.md")]:
            assert (repo/path).read_text() == inputs[key]
    provenance = root / "reused-checkpoints.json"
    if provenance.exists():
        for item in read(provenance)["files"]:
            assert hashlib.sha256((root/item["file"]).read_bytes()).hexdigest() == item["sha256"]
    return {"status":"passed","gate_passed":bool(passed),"phase":inputs["phase"],"specs":10,"articles":20,"checkpoints":requests}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run",type=Path);parser.add_argument("--current",action="store_true")
    args = parser.parse_args();print(json.dumps(verify(args.run,args.current),ensure_ascii=False))
