#!/usr/bin/env python3
"""One-shot continuation of the approved research stages; no scheduled service or retries."""
import fcntl
import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO=Path(__file__).resolve().parents[1]
ROOT=REPO/"research/style-aggregation"
STATE=ROOT/"followthrough-state.json"


def save(**values):
    values.update(pid=os.getpid(),updated_at=datetime.now(timezone.utc).isoformat())
    pending=STATE.with_suffix(".pending");pending.write_text(json.dumps(values,ensure_ascii=False,indent=2)+"\n");pending.replace(STATE)


def ledger_identity():
    entries=json.loads((ROOT/"usage-ledger.json").read_text())["entries"]
    return {k:(v["reserved_micro"],v["actual_micro"]) for k,v in entries.items()}


def main():
    os.chdir(REPO)
    lock=(ROOT/".followthrough.lock").open("a");fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    binary=REPO/"target/research-next/debug/cognitive-writer"
    expected_binary=hashlib.sha256(binary.read_bytes()).hexdigest()
    save(status="waiting",stage="P2",binary_sha256=expected_binary)
    while True:
        state=json.loads((ROOT/"p2-pipeline-state.json").read_text())
        if state["status"]=="complete":break
        if state["status"]!="running":
            save(status="stopped",stage="P2",reason="P2 did not complete; inspect its failure before recovery")
            return 1
        process=Path(f"/proc/{state['pid']}/cmdline")
        if not process.exists() or b"run_style_p2_pipeline.py" not in process.read_bytes():
            save(status="interrupted",stage="P2",reason="P2 worker is absent; running state was stale")
            return 1
        time.sleep(10)
    py=str(ROOT/".plot-env/bin/python")
    research=lambda *args:[str(binary),"research",*args]
    paid_steps=[
        ("profiles",research("profiles","p3-profiles-01","p2-views-02","p2-pairs-02")),
        ("convergence",research("convergence","p4-convergence-01","p3-profiles-01","p2-pairs-02")),
        ("generation",research("generate","p5-generation-01","p3-profiles-01","p2-detailed-single-01")),
        ("reference-evaluation",research("evaluate","p5-reference-01","p5-generation-01","reference")),
        ("confirmation-evaluation",research("evaluate","p5-confirmation-01","p5-generation-01","confirmation","p5-reference-01")),
    ]
    steps=[("P2-analysis",[py,"scripts/analyze_style_p2.py","p2-pairs-02"]),*paid_steps,
           ("P4-report",[py,"scripts/report_style_followthrough.py","convergence","p3-profiles-01","p4-convergence-01"]),
           ("P5-report",[py,"scripts/report_style_followthrough.py","evaluation","p5-reference-01","p5-confirmation-01"])]
    completed=[]
    for stage,argv in steps:
        if hashlib.sha256(binary.read_bytes()).hexdigest()!=expected_binary:
            save(status="stopped",stage=stage,reason="Frozen executable changed",completed=completed);return 1
        save(status="running",stage=stage,completed=completed,binary_sha256=expected_binary)
        print(f"Starting {stage}",flush=True)
        result=subprocess.run(argv,cwd=REPO)
        if result.returncode:
            save(status="stopped",stage=stage,exit_code=result.returncode,completed=completed,binary_sha256=expected_binary)
            print(f"Stopped: {stage}. Do not revise thresholds or discard failed results.",flush=True);return 1
        completed.append(stage)
    # Replay the complete model stages; receipts must satisfy every call without new billing entries.
    before=ledger_identity()
    for stage,argv in paid_steps:
        save(status="running",stage="replay-"+stage,completed=completed,binary_sha256=expected_binary)
        result=subprocess.run(argv,cwd=REPO)
        if result.returncode or ledger_identity()!=before:
            save(status="stopped",stage="replay-"+stage,reason="Replay failed or billing identity changed",completed=completed);return 1
    save(status="data_ready",stage="P6-review",completed=completed,zero_new_request_replay=True,binary_sha256=expected_binary,
         remaining=["Review research conclusions and representative evidence", "Synchronize full results and charts to Lark", "Prepare optional blind feedback forms", "Final report and Git evidence commit"])
    print("P2–P5 data ready and replay checked. P6 review remains; do not claim whole project complete.",flush=True)
    return 0


if __name__=="__main__":
    try:sys.exit(main())
    except Exception as error:
        # Avoid raw subprocess/environment dumps; ordinary local validation errors are sufficient.
        save(status="stopped",stage="worker-error",reason=type(error).__name__)
        print(f"Worker stopped: {type(error).__name__}",flush=True);sys.exit(1)
