#!/usr/bin/env python3
"""One-shot pilot follow-up: bounded recovery, reports and Lark projection; no adoption."""
import argparse
import fcntl
import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime,timezone
from pathlib import Path

REPO=Path(__file__).resolve().parents[1]
ROOT=REPO/"research/style-aggregation"
RUN="p2-candidates-pilot-03"
STATE=ROOT/"candidate-followup-state.json"

def save(**data):
    data.update(pid=os.getpid(),updated_at=datetime.now(timezone.utc).isoformat())
    pending=STATE.with_suffix(".pending");pending.write_text(json.dumps(data,ensure_ascii=False,indent=2)+"\n");pending.replace(STATE)

def process_identity(pid):
    try:
        root=Path("/proc")/str(pid)
        args=(root/"cmdline").read_bytes().split(b"\0")
        if not args or not args[0].endswith(b"/cognitive-writer") or RUN.encode() not in args:return None
        return (root/"stat").read_text().rsplit(")",1)[1].split()[19]
    except (FileNotFoundError,ProcessLookupError):return None

def execute(argv):
    result=subprocess.run(argv,cwd=REPO,timeout=14400)
    if result.returncode:raise RuntimeError(f"Step failed: {Path(argv[0]).name} {argv[1] if len(argv)>1 else ''}")

def main():
    ap=argparse.ArgumentParser();ap.add_argument("--wait-pid",type=int,required=True);args=ap.parse_args()
    with (ROOT/".candidate-followup.lock").open("w") as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        binary=REPO/"target/research-candidates/debug/cognitive-writer"
        binary_sha=hashlib.sha256(binary.read_bytes()).hexdigest()
        identity=process_identity(args.wait_pid)
        save(status="waiting",run=RUN,wait_pid=args.wait_pid,binary_sha256=binary_sha)
        deadline=time.monotonic()+14400
        while identity and process_identity(args.wait_pid)==identity:
            if time.monotonic()>deadline:raise RuntimeError("Pilot still alive after four hours; preserve checkpoints")
            time.sleep(10)
        summary_path=ROOT/"runs"/RUN/"summary.json"
        summary=json.loads(summary_path.read_text())
        recover=summary["status"]=="running" or any("网络请求失败" in r.get("error","") for r in summary.get("rows",[]))
        if recover:
            assert hashlib.sha256(binary.read_bytes()).hexdigest()==binary_sha,"Frozen binary changed"
            save(status="recovering_once",run=RUN,binary_sha256=binary_sha)
            # Store enforces the same three-attempt and 3-CNY cap, including old unknown charges.
            subprocess.run([str(binary),"research","candidate-pilot-v4",RUN,"p2-views-02"],cwd=REPO,timeout=14400)
            summary=json.loads(summary_path.read_text())
        if summary["status"]=="running":raise RuntimeError("Run remains incomplete after recovery; no completion claim")
        save(status="reporting",run=RUN,run_status=summary["status"])
        execute([sys.executable,"scripts/audit_style_candidate_pilot.py",RUN])
        execute([str(ROOT/".plot-env/bin/python"),"scripts/report_style_candidate_progress.py"])
        rows=summary.get("rows",[])
        observation=f"最新小试状态{summary['status']}：{sum(r['status']=='complete' for r in rows)}/6完成、{sum(r['status']=='failed' for r in rows)}失败、{sum(r['status']=='deferred' for r in rows)}暂缓；本轮占用{summary['usage']['run_occupied_cny']:.6f}元。结构结果不等于语义采用。"
        journal=ROOT/"journal.md";text=journal.read_text()
        if "## E0024 ·" not in text:
            journal.write_text(text.rstrip()+"\n\n## E0024 · 2026-10-09 · 小试自动收尾（execution_snapshot）\n\n"+observation+"后续须复核语义及实测成本，自主决定方法；不自动启动完整矩阵。详见candidate-followup-state.json与阶段报告。\n")
        events_path=ROOT/"lark/journal-records.json";events=json.loads(events_path.read_text())
        if not any(r["event_id"]=="E0024" for r in events):
            events.append(dict(event_id="E0024",kind="小试收尾",observation=observation,meaning="不是作者风格或聚合效果结论。",next_step="自主复核含义、成本和可扩展性；不需要用户确认。"))
            events_path.write_text(json.dumps(events,ensure_ascii=False,indent=2)+"\n")
        current=REPO/".claude/tasks/current.md";body=current.read_text()
        heading="## 2026-10-09 小试自动收尾快照"
        if heading not in body:
            current.write_text(body.replace("# 当前任务状态\n","# 当前任务状态\n\n"+heading+"\n\n"+observation+"\n自动收尾只到报告/投影，下一步语义复核后自主执行；不等同完整研究结束。最新同步/归档状态见candidate-followup-state.json。\n",1))
        execute([sys.executable,"scripts/publish_style_candidate_progress.py"])
        runs=["p2-candidates-pilot-01","p2-candidates-pilot-02","p2-candidate-cases-01",RUN]
        execute([sys.executable,"scripts/sync_style_research_lark.py","--runs",*[str(ROOT/"runs"/r) for r in runs]])
        save(status="reported_review_pending",run=RUN,run_status=summary["status"],meaning="需执行者复核，不是等待用户确认")
        # Commit only this research follow-up's artifacts; never stage source or unrelated user work.
        owned=[f"research/style-aggregation/runs/{RUN}","research/style-aggregation/usage-ledger.json",
            "research/style-aggregation/journal.md","research/style-aggregation/lark","research/style-aggregation/candidate-followup-state.json",
            "research/style-aggregation/reports/p2-candidates-pilot-03","research/style-aggregation/reports/candidate-progress-2026-10-11",
            "research/style-aggregation/charts/candidate-progress-2026-10-11",".claude/tasks/current.md"]
        execute(["git","add","--",*owned]);execute(["git","diff","--cached","--check"])
        staged=subprocess.check_output(["git","diff","--cached","--name-only"],cwd=REPO,text=True).splitlines()
        if any(not any(p==a or p.startswith(a+"/") for a in owned) for p in staged):
            raise RuntimeError("Unrelated staged changes found; preserve staging and do not commit")
        if staged:execute(["git","commit","-m","Archive bounded candidate pilot results and research report"])

if __name__=="__main__":
    try:main()
    except BlockingIOError:
        raise SystemExit("Another candidate follow-up owns the lock; existing state preserved")
    except Exception as error:
        save(status="stopped",error=str(error),run=RUN)
        raise
