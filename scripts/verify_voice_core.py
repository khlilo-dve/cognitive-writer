#!/usr/bin/env python3
"""Verify core artifacts and provenance; semantic acceptance is deliberately separate."""
import hashlib
import json
from pathlib import Path

ROOT = Path("research/style-aggregation/runs/voice-learning-core-01")


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def main():
    plan = json.loads((ROOT / "inputs.json").read_text())
    summary = json.loads((ROOT / "summary.json").read_text())
    assert summary["status"] == "complete" and len(summary["rows"]) == 6
    rows = []
    for job in plan["jobs"]:
        ident = job["id"]
        saved = json.loads((ROOT / "profiles" / f"{ident}.json").read_text())
        result = saved["result"]
        assert saved["source_ids"] == job["source_ids"]
        assert result["status"] == "draft_pending_semantic_review"
        assert result["method"] == "voice-core-v1"
        assert result["spec"] == (ROOT / "profiles" / f"{ident}.md").read_text()
        assert result["analysis"] == (ROOT / "analyses" / f"{ident}.md").read_text()
        assert {s["text_sha256"] for s in result["sources"]} == {digest(t) for t in job["texts"]}
        normalized = [digest("".join(c for c in t if not c.isspace())) for t in job["texts"]]
        assert [s["normalized_sha256"] for s in result["sources"]] == sorted(normalized)
        assert [s["article"] for s in result["sources"]] == [f"a{i+1:02}" for i in range(len(normalized))]
        rows.append({"id": ident, "source_count": len(result["sources"]),
                     "spec_sha256": digest(result["spec"]), "fingerprint": result["fingerprint"]})
    assert sorted(r["source_count"] for r in rows) == [1, 1, 3, 3, 10, 10]
    replay = json.loads((ROOT / "replay-audit.json").read_text())
    assert replay["status"] == "passed" and replay["new_requests"] == 0
    (ROOT / "artifact-audit.json").write_text(json.dumps({
        "status": "passed", "semantic_quality": "separate_review_required", "rows": rows,
        "zero_new_request_replay": True}, ensure_ascii=False, indent=2) + "\n")
    print("Verified six drafts, source identities, canonical order and cached replay")


if __name__ == "__main__":
    main()
