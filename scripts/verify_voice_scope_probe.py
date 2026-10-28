#!/usr/bin/env python3
"""Verify the frozen 18-draft design and identities; factual judgments remain a separate readout."""
import hashlib
import json
from pathlib import Path

ROOT = Path("research/style-aggregation/runs/voice-learning-scope-probe-01")


def main():
    inputs = json.loads((ROOT / "inputs.json").read_text())
    summary = json.loads((ROOT / "summary.json").read_text())
    assert summary["status"] == "complete" and len(summary["rows"]) == 18
    jobs = inputs["jobs"]
    assert len({j["id"] for j in jobs}) == 18
    expected = {(s, c, t) for s in ("wxb-k1", "wxb-k10", "wzq-k1", "wzq-k10")
                for c in ("before", "after") for t in ("map_drawer", "read_later")}
    expected |= {("neutral", "control", t) for t in ("map_drawer", "read_later")}
    actual = {(j["style"]["style_id"], j["style"]["condition"], j["topic"]) for j in jobs}
    assert actual == expected
    policy = Path("research/style-aggregation/prompts/writer-closed-facts-v1.md").read_text()
    rows = []
    for job in jobs:
        ident = job["id"]
        saved = json.loads((ROOT / "articles" / f"{ident}.json").read_text())
        request = json.loads((ROOT / "requests" / f"{ident}.json").read_text())
        assert saved["job"] == job
        assert saved["body"] == (ROOT / "articles" / f"{ident}.md").read_text()
        assert saved["body"] == request["receipt"]["content"]
        assert request["receipt"]["model"] == "mimo-v2.6-flash"
        assert job["system"].endswith(policy)
        assert request["request"]["body"]["messages"] == [
            {"role": "system", "content": job["system"]},
            {"role": "user", "content": job["user"]}]
        rows.append({"id": ident, "body_sha256": hashlib.sha256(saved["body"].encode()).hexdigest()})
    (ROOT / "artifact-audit.json").write_text(json.dumps({
        "status": "artifact_identity_verified", "same_writer_fact_policy": True,
        "semantic_quality": "separate_qualitative_review", "rows": rows},
        ensure_ascii=False, indent=2) + "\n")
    print("Verified 18 frozen before/after/control drafts, uniform policy and raw-response identities")


if __name__ == "__main__":
    main()
