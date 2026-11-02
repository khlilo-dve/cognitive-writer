#!/usr/bin/env python3
"""Audit frozen new-topic inputs, raw receipts and source specs; no semantic scoring or API calls."""
import argparse
import hashlib
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
RUN = ROOT / "research/style-aggregation/runs/learning-quality-probe-01"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main(boundary=False):
    run_id = "learning-boundary-probe-01" if boundary else "learning-quality-probe-01"
    source_id = "learning-grounded-repair-01" if boundary else "learning-jobs-quality-01"
    RUN = ROOT / "research/style-aggregation/runs" / run_id
    inputs = json.loads((RUN / "inputs.json").read_text())
    summary = json.loads((RUN / "summary.json").read_text())
    count = 4 if boundary else 10
    assert summary["status"] == "complete" and len(summary["rows"]) == count
    jobs = inputs["jobs"]
    expected = {(s, t) for s in ["wxb-k1", "wxb-k2", "wzq-k1", "wzq-k2", "neutral"]
                for t in ["tool_board", "photo_numbers"]}
    if boundary:
        expected = {(s,"abstract_choices") for s in ["wxb-k1","wxb-k2"]} | {(s,"given_feeling") for s in ["wzq-k1","wzq-k2"]}
    assert len(jobs) == count and {(j["style"]["id"], j["topic"]) for j in jobs} == expected
    suite_file = ROOT / "research/style-aggregation/datasets" / ("learning-boundary-tasks-v1.json" if boundary else "learning-quality-tasks-v1.json")
    assert inputs["tasks_sha256"] == digest(suite_file.read_bytes())
    suite = json.loads(suite_file.read_text())
    policy = (ROOT / "research/style-aggregation/prompts/writer-closed-facts-v1.md").read_text()
    ledger = ROOT / "research/style-aggregation/usage-ledger.json"
    before = digest(ledger.read_bytes())
    rows = []
    for job in jobs:
        ident = job["id"]
        cp = json.loads((RUN / "requests" / f"{ident}.json").read_text())
        saved = json.loads((RUN / "articles" / f"{ident}.json").read_text())
        assert saved["job"] == job and saved["body"] == cp["receipt"]["content"]
        assert saved["body"] == (RUN / "articles" / f"{ident}.md").read_text()
        assert cp["receipt_sha256"] == digest(json.dumps(cp["receipt"],ensure_ascii=False,separators=(",", ":")).encode())
        assert cp["receipt"]["model"] == "mimo-v2.6-flash" and cp["receipt"]["finish_reason"] == "stop"
        assert cp["request"]["body"]["messages"] == [
            {"role":"system","content":job["system"]},{"role":"user","content":job["user"]}]
        assert job["system"].endswith(policy)
        topic = next(t for t in suite["tasks"] if t["id"] == job["topic"])
        assert topic["facts"] in job["user"] and topic["requirements"] in job["user"]
        style = job["style"]
        if style["id"] != "neutral":
            code = style["id"].split("-")[0]
            source = ROOT / "research/style-aggregation/runs" / style["source_run"]
            assert source.name == source_id
            candidate = json.loads((source/"workspaces"/code/"candidates"/f"{style['candidate_id']}.json").read_text())
            assert candidate["input"]["origin"] == "model_output"
            draft = candidate["input"]["draft"]
            assert style["spec"] == draft["spec"] and style["fingerprint"] == draft["fingerprint"]
            assert draft["method"] == "voice-reviewed-v3"
            if boundary:
                plan = json.loads((source/"workspaces"/code/"jobs"/style["job_id"]/"plan.json").read_text())
                assert plan["learning_method"] == "voice-core-v6"
            assert draft["spec"] in job["system"]
            assert draft["analysis"] not in job["system"] and candidate["input"]["review"] not in job["system"]
        else:
            assert style["spec"] == ""
        rows.append({"id":ident,"body_sha256":digest(saved["body"].encode()),"model":cp["receipt"]["model"]})
    assert before == digest(ledger.read_bytes())
    (RUN / "artifact-audit.json").write_text(json.dumps({"status":"passed","rows":rows,
        "new_model_requests":0,"same_tasks_and_fact_policy":True,"new_automatic_candidate_identity_verified":True,
        "analysis_and_review_excluded_from_writer":True,"ledger_before_sha256":before,"ledger_after_sha256":before,
        "semantic_and_factual_quality":"separate_implementer_readout_required"},ensure_ascii=False,indent=2)+"\n")
    print(f"Verified {count} Flash receipts, uniform tasks/policy and new automatic style identities; no requests")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--boundary", action="store_true")
    main(parser.parse_args().boundary)
