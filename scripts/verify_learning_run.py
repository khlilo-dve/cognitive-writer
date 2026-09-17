#!/usr/bin/env python3
"""Offline integrity check for a completed single-article comparison; no API calls."""
import argparse
import hashlib
import json
from pathlib import Path


def check(run: Path, current: bool = False) -> dict:
    summary = json.loads((run / "summary.json").read_text())
    inputs = json.loads((run / "inputs.json").read_text())
    assert summary["status"] == "complete", "run is incomplete"
    assert summary["model"] == inputs["model"]
    rows = summary["rows"]
    candidate = "direct_free" if "direct_prompt" in inputs else "two_stage"
    expected = {(i, method) for i in range(1, 6) for method in ("single_stage", candidate)}
    assert len(rows) == 10 and {(r["case"], r["variant"]) for r in rows} == expected
    checkpoints = 0
    for row in rows:
        name = f"s{row['case']:02}_{row['variant']}"
        spec = (run / row["spec"]).read_text()
        count = sum(not char.isspace() for char in spec)
        assert count == row["judgment"]["characters_without_whitespace"]
        assert (run / row["body"]).read_text().strip()
        for suffix in ("judge", "outline", "body", "transfer"):
            cp = json.loads((run / f"{name}_{suffix}.json").read_text())
            assert cp["request"]["model"] == inputs["model"] and cp["response"].strip()
            assert set(cp["request"]) == {"schema", "endpoint_fingerprint", "model", "system", "user"}
            if suffix == "body":
                assert cp["response"] == (run / row["body"]).read_text()
            if suffix == "judge":
                assert cp["request"]["system"] == inputs["judge"]
                assert spec in cp["request"]["user"]
            checkpoints += 1
        if row["variant"] == "single_stage":
            cp = json.loads((run / f"{name}_extract.json").read_text())
            assert cp["request"]["system"] == inputs["old_prompt"]
            assert cp["request"]["user"] == inputs["samples"][row["case"] - 1]
            assert cp["response"] == spec
            checkpoints += 1
        else:
            learned = json.loads((run / f"{name}_learning.json").read_text())
            assert learned["spec"] == spec
            stages = sorted(run.glob(f"{name}_analysis_*.json")) + sorted(run.glob(f"{name}_compile_*.json")) + sorted(run.glob(f"{name}_direct_*.json"))
            assert len(stages) == learned["calls"] == row["extraction_calls"]
            for stage in stages:
                cp = json.loads(stage.read_text())
                which = "analyze" if "_analysis_" in stage.name else "compile"
                expected_prompt = inputs["direct_prompt"] if candidate == "direct_free" else inputs["new_prompts"][which]
                assert cp["request"]["system"] == expected_prompt
                assert cp["request"]["model"] == inputs["model"]
            if candidate == "two_stage":
                clean = json.loads(json.loads((run / f"{name}_compile_0.json").read_text())["request"]["user"])
                assert all(set(f) == {"rule", "condition", "support"} for f in clean["features"])
            else:
                assert learned["analysis"] is None
            checkpoints += len(stages)
    provenance = run / "reused-checkpoints.json"
    if provenance.exists():
        for item in json.loads(provenance.read_text())["files"]:
            assert hashlib.sha256((run / item["file"]).read_bytes()).hexdigest() == item["sha256"]
    if current:
        repo = Path(__file__).resolve().parents[1]
        if candidate == "direct_free":
            assert (repo / "prompts/learn_style_direct.md").read_text() == inputs["direct_prompt"]
        assert (repo / "prompts/analyze_style.md").read_text() == inputs["new_prompts"]["analyze"]
        assert (repo / "prompts/learn_style.md").read_text() == inputs["new_prompts"]["compile"]
        assert (repo / "prompts/eval_judge_learn.md").read_text() == inputs["judge"]
        assert (repo / "prompts/eval_judge_transfer.md").read_text() == inputs["transfer_judge"]
    return {"status": "passed", "run": run.name, "rows": len(rows), "checkpoints": checkpoints,
            "model": summary["model"], "current_prompts_verified": current}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--current", action="store_true")
    args = parser.parse_args()
    print(json.dumps(check(args.run, args.current), ensure_ascii=False))
