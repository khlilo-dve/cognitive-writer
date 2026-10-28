#!/usr/bin/env python3
"""Apply a finite, explicit implementer edit set to new artifacts, preserving model outputs."""
import difflib
import hashlib
import json
from pathlib import Path

RUNS = Path("research/style-aggregation/runs")
TARGET = RUNS / "voice-learning-scope-reviewed-01"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def save(path, data):
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def main():
    edits_bytes = (TARGET / "edits.json").read_bytes()
    config = json.loads(edits_bytes)
    source = RUNS / config["source_run"]
    summary = json.loads((source / "summary.json").read_text())
    assert summary["status"] == "complete" and len(summary["rows"]) == 6
    frozen = {"source_run": config["source_run"], "edit_set_sha256": digest(edits_bytes),
              "parent_files": {row["id"]: digest((source / "profiles" / f"{row['id']}.json").read_bytes())
                               for row in summary["rows"]}}
    frozen_path = TARGET / "inputs.json"
    if frozen_path.exists():
        assert json.loads(frozen_path.read_text()) == frozen, "Edit inputs changed; create a new derived run"
    else:
        save(frozen_path, frozen)
    ledger = Path("research/style-aggregation/usage-ledger.json")
    ledger_before = digest(ledger.read_bytes())
    for folder in ("profiles", "diffs"):
        (TARGET / folder).mkdir(exist_ok=True)
    rows = []
    for row in summary["rows"]:
        ident = row["id"]
        saved = json.loads((source / "profiles" / f"{ident}.json").read_text())
        before = saved["result"]["spec"]
        assert before == (source / "profiles" / f"{ident}.md").read_text()
        after = before
        selected = config["edits"].get(ident, [])
        for edit in selected:
            assert after.count(edit["before"]) == 1, (ident, edit["before"])
            after = after.replace(edit["before"], edit["after"], 1)
        result = dict(saved["result"], spec=after, method="voice-reviewed-v2+implementer-edits-v1")
        result["fingerprint"] = digest(json.dumps({
            "parent_fingerprint": saved["result"]["fingerprint"],
            "parent_spec_sha256": digest(before.encode()), "edits_sha256": digest(edits_bytes),
            "spec_sha256": digest(after.encode())}, sort_keys=True).encode())
        result["warnings"].append("含实现者有限定点修订，不能用来证明全自动方法已解决这些问题；未独立人评。")
        save(TARGET / "profiles" / f"{ident}.json", {
            "id": ident, "source_ids": saved["source_ids"], "source_run": config["source_run"],
            "review_type": config["review_type"], "edits": selected,
            "parent_spec_sha256": digest(before.encode()), "result": result})
        (TARGET / "profiles" / f"{ident}.md").write_text(after)
        (TARGET / "diffs" / f"{ident}.diff").write_text("".join(difflib.unified_diff(
            before.splitlines(True), after.splitlines(True), fromfile=f"model/{ident}",
            tofile=f"reviewed/{ident}", n=0)))
        rows.append({"id": ident, "status": "complete", "edits": len(selected),
                     "spec_sha256": digest(after.encode())})
    assert digest(ledger.read_bytes()) == ledger_before
    save(TARGET / "summary.json", {"status": "complete", "stage": "implementer_edits",
        "rows": rows, "new_requests": 0, "usage": {"run_occupied_cny": 0.0}})
    save(TARGET / "artifact-audit.json", {"status": "passed", "new_requests": 0,
        "ledger_sha256": ledger_before, "review_type": config["review_type"], "rows": rows})
    print(f"Applied {sum(r['edits'] for r in rows)} explicit edits; six parent artifacts preserved; zero requests")


if __name__ == "__main__":
    main()
