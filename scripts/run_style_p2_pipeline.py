#!/usr/bin/env python3
"""Sequential P2 worker. Stops on any failed stage; never changes prompts or retries."""
import fcntl
import hashlib
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
ROOT = REPO / "research/style-aggregation"
STATE = ROOT / "p2-pipeline-state.json"


def save(**values):
    values.update(updated_at=datetime.now(timezone.utc).isoformat(), pid=os.getpid())
    pending = STATE.with_suffix(".pending")
    pending.write_text(json.dumps(values, ensure_ascii=False, indent=2) + "\n")
    pending.replace(STATE)


def main():
    os.chdir(REPO)
    lock = (ROOT / ".pipeline.lock").open("a")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    for name in ("p2-detailed-single-01", "p2-detailed-repeat-01"):
        summary = json.loads((ROOT / "runs" / name / "summary.json").read_text())
        if summary["status"] != "complete":
            raise RuntimeError(f"Prerequisite incomplete: {name}")
    binary = str(REPO / "target/research-next/debug/cognitive-writer")
    steps = [
        ("views", [binary, "research", "views", "p2-views-02", "p2-detailed-single-01", "p2-detailed-repeat-01"]),
        ("pairs", [binary, "research", "pairs", "p2-pairs-02", "p2-views-02"]),
        ("figures", [str(ROOT / ".plot-env/bin/python"), "scripts/plot_style_research.py", "p2-pairs-02"]),
    ]
    provenance = {"binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
                  "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    completed = []
    for stage, argv in steps:
        save(status="running", stage=stage, completed=completed, **provenance)
        print(f"Starting {stage}", flush=True)
        # Inherit only stdout/stderr; model transport already removes credentials and raw errors.
        result = subprocess.run(argv, cwd=REPO)
        if result.returncode:
            save(status="stopped", stage=stage, exit_code=result.returncode,
                 completed=completed, **provenance)
            print(f"Stopped at {stage}; inspect saved results before any recovery", flush=True)
            return 1
        completed.append(stage)
    save(status="complete", stage="P2-data-ready", completed=completed, **provenance)
    print("P2 data and figures ready; interpretation and later phases remain", flush=True)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"Pipeline preflight failed: {type(error).__name__}: {error}", flush=True)
        sys.exit(1)
