#!/usr/bin/env python3
"""Audit scope revision artifacts and locate quoted spans, without declaring semantic success."""
import difflib
import hashlib
import json
import re
from pathlib import Path

ROOT = Path("research/style-aggregation/runs/voice-learning-scope-recovered-01")


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def main():
    plan = json.loads((ROOT / "inputs.json").read_text())
    summary = json.loads((ROOT / "summary.json").read_text())
    assert summary["status"] == "complete" and len(summary["rows"]) == 6
    out = ROOT / "diffs"
    out.mkdir(exist_ok=True)
    rows = []
    for job in plan["jobs"]:
        ident = job["id"]
        saved = json.loads((ROOT / "profiles" / f"{ident}.json").read_text())
        result = saved["result"]
        previous = job["draft"]
        assert saved["parent_spec_sha256"] == digest(previous["spec"])
        assert saved["source_ids"] == job["source_ids"]
        assert result["sources"] == previous["sources"]
        assert result["spec"] == (ROOT / "profiles" / f"{ident}.md").read_text()
        assert saved["review"] == (ROOT / "reviews" / f"{ident}.md").read_text()
        assert result["status"] == "draft_pending_semantic_review"
        assert result["method"] == "voice-reviewed-v2"
        assert result["calls"] == previous["calls"] + saved["revision_calls"]
        request = json.loads((ROOT / "requests" / f"{ident}-review.json").read_text())
        user = json.loads(request["request"]["body"]["messages"][-1]["content"])
        assert set(user) == {"articles", "candidate"}
        assert user["candidate"] == previous["spec"]
        assert {digest(a["text"]) for a in user["articles"]} == {digest(t) for t in job["texts"]}
        diff = "".join(difflib.unified_diff(
            previous["spec"].splitlines(True), result["spec"].splitlines(True),
            fromfile=f"before/{ident}", tofile=f"after/{ident}", n=0))
        (out / f"{ident}.diff").write_text(diff)
        quotes = []
        for match in re.finditer(r'"([^"\n]+)"|“([^”\n]+)”', saved["review"]):
            text = (match.group(1) or match.group(2)).replace("**", "")
            if len(text) < 6:
                continue
            quotes.append({"text": text,
                           "source_matches": [a["article"] for a in user["articles"] if text in a["text"]],
                           "candidate_match": text in previous["spec"]})
        rows.append({"id": ident, "style_sha256": digest(result["spec"]),
                     "review_sha256": digest(saved["review"]), "quotes": quotes})
    (ROOT / "artifact-audit.json").write_text(json.dumps({
        "status": "artifact_identity_verified", "review_context": "sources_and_candidate_only",
        "quote_note": "Unmatched spans can be suggestions, abbreviated or erroneous quotes; inspect in context.",
        "semantic_quality": "requires_separate_reading", "rows": rows},
        ensure_ascii=False, indent=2) + "\n")
    print("Verified six revisions, preserved parents/sources, isolated review inputs and deterministic diffs")


if __name__ == "__main__":
    main()
