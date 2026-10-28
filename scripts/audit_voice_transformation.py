#!/usr/bin/env python3
"""Record deterministic analysis-to-Style diffs without model-authored deletion claims."""
import difflib
import hashlib
import json
from pathlib import Path

ROOT = Path("research/style-aggregation/runs")
SOURCE = ROOT / "voice-learning-prose-01"
TARGET = ROOT / "voice-learning-clean-pro-01"


def main():
    assert json.loads((TARGET / "summary.json").read_text())["status"] == "complete"
    output = TARGET / "transformation-diffs"
    output.mkdir(exist_ok=True)
    rows = []
    for ident in ("wxb-k1", "wxb-k2", "wzq-k1", "wzq-k2"):
        before = (SOURCE / "profiles" / f"{ident}.md").read_text()
        after = (TARGET / "profiles" / f"{ident}.md").read_text()
        saved = json.loads((TARGET / "profiles" / f"{ident}.json").read_text())
        assert saved["result"]["spec"] == after
        assert saved["draft_sha256"] == hashlib.sha256(before.encode()).hexdigest()
        diff = "".join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                           fromfile=f"analysis/{ident}", tofile=f"style/{ident}", n=0))
        (output / f"{ident}.diff").write_text(diff)
        rows.append({"id": ident, "analysis_sha256": saved["draft_sha256"],
                     "style_sha256": hashlib.sha256(after.encode()).hexdigest(),
                     "diff_path": f"transformation-diffs/{ident}.diff"})
    (TARGET / "transformation-audit.json").write_text(json.dumps({
        "status": "artifact_identity_verified", "semantic_quality": "requires_review",
        "rows": rows}, ensure_ascii=False, indent=2) + "\n")
    print("Verified four transformations and wrote deterministic diffs")


if __name__ == "__main__":
    main()
