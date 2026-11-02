#!/usr/bin/env python3
"""Verify real development jobs and recover completed candidates through the product CLI, offline."""
import argparse
import hashlib
import re
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUN = ROOT / "research/style-aggregation/runs/learning-jobs-live-02"
BIN = ROOT / "target/debug/cognitive-writer"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def identity(record):
    return digest(json.dumps({k: v for k, v in record.items() if k != "id"},
                             ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode())


def main(run_id="learning-jobs-live-02"):
    assert re.fullmatch(r"[A-Za-z0-9_-]+", run_id)
    RUN = ROOT / "research/style-aggregation/runs" / run_id
    summary = json.loads((RUN / "summary.json").read_text())
    assert summary["status"] == "complete" and len(summary["rows"]) == 2
    ledger = ROOT / "research/style-aggregation/usage-ledger.json"
    before = digest(ledger.read_bytes())
    versions_before = {str(p): digest(p.read_bytes()) for p in RUN.glob("workspaces/*/versions/*/*.json")}
    rows = []
    export_dir = RUN / "validation-exports"
    export_dir.mkdir(exist_ok=True)
    for chain in summary["rows"]:
        code = chain["id"]
        assert chain["status"] == "complete" and len(chain["jobs"]) == 2
        ws = RUN / "workspaces" / code
        previous = None
        for item in chain["jobs"]:
            count, job_id = item["sources"], item["job_id"]
            job_dir = ws / "jobs" / job_id
            plan = json.loads((job_dir / "plan.json").read_text())
            state = json.loads((job_dir / "state.json").read_text())
            assert plan["id"] == job_id == identity(plan)
            assert plan["profile"]["model"] == "mimo-v2.6-pro"
            assert len(plan["articles"]) == count and count in (1, 2)
            assert state["status"] == "complete" and state["candidate_id"] == item["candidate_id"]
            candidate = json.loads((ws / "candidates" / f"{item['candidate_id']}.json").read_text())
            assert candidate["id"] == identity(candidate)
            assert candidate["input"]["articles"] == plan["articles"]
            assert candidate["input"]["draft"]["status"] == "draft_pending_semantic_review"
            received = list((job_dir / "requests").glob("*.response.json"))
            assert 4 <= len(received) <= 7
            for path in received:
                stage = path.name.removesuffix(".response.json")
                saved = json.loads(path.read_text())
                assert saved["response_sha256"] == digest(json.dumps(saved["response"],
                    ensure_ascii=False, separators=(",", ":")).encode())
                origin = json.loads((RUN / "requests" / f"{code}-k{count}-{stage}.json").read_text())
                assert saved["request"] == origin["request"]
                if stage == "learn-compile-0" and plan["learning_method"] == "voice-core-v3":
                    payload = json.loads(saved["request"]["body"]["messages"][1]["content"])
                    assert [a["text"] for a in payload["articles"]] == plan["articles"]
                    assert payload["analysis"] == candidate["input"]["draft"]["analysis"]
                if stage == "learn-compile-0" and plan["learning_method"] in ("voice-core-v4", "voice-core-v5", "voice-core-v6"):
                    payload = json.loads(saved["request"]["body"]["messages"][1]["content"])
                    assert set(payload)=={"article_count","analysis"}
                    assert payload["analysis"] == candidate["input"]["draft"]["analysis"]
                assert saved["response"] == origin["receipt"]
                assert saved["response"]["model"] == "mimo-v2.6-pro"
            if plan["learning_method"] in ("voice-core-v5", "voice-core-v6"):
                paths = sorted((job_dir/"requests").glob("learn-analysis*.response.json"))
                valid = []
                for p in paths:
                    record=json.loads(p.read_text())
                    try:
                        report=json.loads(record["response"]["content"])
                        source_user=json.loads(record["request"]["body"]["messages"][1]["content"])
                        articles=source_user.get("articles",source_user.get("original_input",{}).get("articles"))
                        lookup={a["article"]:a["text"] for a in articles}
                        assert report["analysis"] == candidate["input"]["draft"]["analysis"]
                        assert set(lookup) <= {e["article"] for e in report["evidence"]}
                        assert all(e["quote"] and e["quote"] in lookup[e["article"]] for e in report["evidence"])
                        valid.append(p.name)
                    except (ValueError,KeyError,AssertionError):
                        pass
                assert valid, "No source-grounded analysis supports candidate"
            assert candidate["input"]["draft"]["calls"] == len(received)
            version = next(p for p in ws.glob(f"versions/*/v{item['version']:06}.json"))
            record = json.loads(version.read_text())
            assert record["id"] == identity(record)
            if previous:
                assert record["previous"] == previous["id"]
                assert candidate["input"]["base"]["id"] == previous["id"]
                assert set(previous["candidate"]["input"]["articles"]) <= set(plan["articles"])
            previous = record
            # Completed candidate recovery works even without a usable API key.
            env = dict(os.environ, API_KEY="", BASE_URL="https://api.xiaomimimo.com/v1")
            result = subprocess.run([str(BIN), "style-draft", "--root", str(ws), "resume", job_id],
                cwd=ROOT, env=env, capture_output=True, text=True, timeout=20)
            assert result.returncode == 0, result.stderr
            assert f"candidate_id: {candidate['id']}" in result.stdout
            out = export_dir / f"{code}-v{item['version']}.md"
            expected = candidate["input"]["draft"]["spec"].encode()
            if not out.exists():
                result = subprocess.run([str(BIN), "style-draft", "--root", str(ws), "export",
                    record["name"], str(item["version"]), str(out)], cwd=ROOT, env=env,
                    capture_output=True, text=True, timeout=20)
                assert result.returncode == 0, result.stderr
            assert out.read_bytes() == expected
            rows.append({"author_chain": code, "sources": count, "job_id": job_id,
                "candidate_id": candidate["id"], "version_id": record["id"], "received_requests": len(received),
                "spec_sha256": digest(expected), "completed_cli_resume_without_credentials": True})
    assert versions_before == {str(p): digest(p.read_bytes()) for p in RUN.glob("workspaces/*/versions/*/*.json")}
    after = digest(ledger.read_bytes())
    assert before == after
    (RUN / "artifact-audit.json").write_text(json.dumps({"status": "passed", "rows": rows,
        "new_model_requests_during_audit": 0, "saved_versions_unchanged": True,
        "ledger_before_sha256": before, "ledger_after_sha256": after,
        "quality": "separate_semantic_and_writer_validation_required"}, ensure_ascii=False, indent=2) + "\n")
    print("Verified four real jobs, request/receipt identities, parent versions and keyless CLI recovery; no new charges")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", default="learning-jobs-live-02")
    main(parser.parse_args().run)
