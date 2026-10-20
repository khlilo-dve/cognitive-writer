#!/usr/bin/env python3
"""Idempotent, sequential CLI projection of local research artifacts into the user's Base."""
import argparse
import json
import subprocess
import time
from pathlib import Path

ROOT = Path("research/style-aggregation")
STATE = ROOT / "lark/resources.json"
CLI = ["/home/khlilo/.nvm/versions/node/v20.20.1/bin/node",
       "/home/khlilo/.nvm/versions/node/v20.20.1/bin/lark-cli"]


def save(state):
    pending = STATE.with_suffix(".pending")
    pending.write_text(json.dumps(state, ensure_ascii=False, indent=2) + "\n")
    pending.replace(STATE)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", type=Path)
    ap.add_argument("--run", type=Path)
    ap.add_argument("--runs", nargs="*", type=Path, default=[])
    args = ap.parse_args()
    state = json.loads(STATE.read_text())
    base = state["base_token"]
    state.setdefault("records", {})

    def call(command, **kwargs):
        argv = CLI + ["base", command, "--base-token", base, "--as", "user"]
        for key, value in kwargs.items():
            argv += ["--" + key.replace("_", "-"), json.dumps(value, ensure_ascii=False)
                     if isinstance(value, (dict, list)) else str(value)]
        result = subprocess.run(argv, capture_output=True, text=True, timeout=90)
        try:
            parsed = json.loads(result.stdout)
        except ValueError:
            raise RuntimeError(f"{command}: CLI did not return JSON")
        if result.returncode or not parsed.get("ok", False):
            raise RuntimeError(f"{command}: {parsed.get('error')}")
        return parsed["data"]

    tables = {
        "Runs": ("Runs｜实验总览", [("run_id", "text"), ("stage", "text"), ("status", "text"),
                                   ("question", "text"), ("conclusion", "text"), ("caveat", "text"), ("spent_cny", "number"), ("report_url", "text")]),
        "Articles": ("Articles｜样本清单", [("article_id", "text"), ("author", "text"), ("title", "text"),
                                          ("topic", "text"), ("split", "text"), ("characters", "number"), ("source_url", "text")]),
        "Journal": ("Journal｜突破与失败", [("event_id", "text"), ("kind", "text"), ("observation", "text"),
                                           ("meaning", "text"), ("next_step", "text")]),
        "Styles": ("Styles｜单篇风格", [("artifact_id", "text"), ("run_id", "text"), ("article_id", "text"),
                                      ("status", "text"), ("characters", "number"), ("warnings", "text"), ("spec", "text")]),
    }
    schemas = {}
    def discover_record_ids(table):
        offset = 0
        primary = tables[table][1][0][0]
        while True:
            data = call("+record-list", table_id=state["tables"][table], offset=offset, limit=200)
            names = data.get("fields", [])
            rows = data.get("data", [])
            ids = data.get("record_id_list", [])
            if rows and (primary not in names or len(rows) != len(ids)):
                raise ValueError("Cannot reconcile remote record IDs")
            for rid, row in zip(ids, rows):
                key = row[names.index(primary)]
                if key:
                    identity = f"{table}/{key}"
                    if identity in state["records"] and state["records"][identity] != rid:
                        raise ValueError("Duplicate remote research artifact ID")
                    state["records"][identity] = rid
            if not data.get("has_more"):
                break
            if not rows:
                raise ValueError("Empty remote pagination result")
            offset += len(rows)
        save(state)

    for key, (name, fields) in tables.items():
        if key not in state["tables"]:
            data = call("+table-create", name=name, fields=[{"name": n, "type": t} for n, t in fields])
            state["tables"][key] = data["table"]["id"]
            save(state)
        data = call("+field-list", table_id=state["tables"][key], limit=100)
        (ROOT / "lark" / f"fields-{key}.json").write_text(json.dumps(data, ensure_ascii=False, indent=2))
        schemas[key] = data
        discover_record_ids(key)
        print("schema", key, flush=True)

    def upsert(table, key, fields):
        # Remote field names were read above; reject any mismatch before mutation.
        stored_fields = {f["field_name"]: f["type"] for f in schemas[table]["items"]}
        url_fields = {f["field_name"] for f in schemas[table]["items"] if f.get("style", {}).get("type") == "url"}
        for field in fields:
            if field not in stored_fields or stored_fields[field] not in ("text", "number"):
                raise ValueError(f"Unknown remote field: {field}")
        identity = f"{table}/{key}"
        kwargs = {"table_id": state["tables"][table], "json": fields}
        if identity in state["records"]:
            kwargs["record_id"] = state["records"][identity]
        data = call("+record-upsert", **kwargs)
        record = data["record"]
        rid = kwargs.get("record_id") or record.get("id") or record.get("record_id")
        if not rid:
            # New writes can take a moment to become visible to record-list.
            # Retry only the read; never repeat the create operation.
            for delay in (0.5, 1, 2, 4):
                time.sleep(delay)
                discover_record_ids(table)
                rid = state["records"].get(identity)
                if rid:
                    break
        if not rid:
            raise ValueError("Record response missing ID; stop before duplicate writes")
        state["records"][identity] = rid
        save(state)
        # A successful write may become visible after the first read. Retry reads only.
        for delay in (0, 0.5, 1, 2, 4):
            if delay:
                time.sleep(delay)
            verified = call("+record-get", table_id=state["tables"][table], record_id=rid)["record"]
            # Base normalizes empty text to null and styled URLs to Markdown links.
            if not any(verified.get(k) != v and not (stored_fields[k] == "text" and v == "" and verified.get(k) is None)
                       and not (k in url_fields and v and verified.get(k) == f"[{v}]({v})")
                       for k, v in fields.items()):
                return verified
        raise ValueError(f"Remote record differs after bounded readback: {identity}")

    if args.manifest:
        ds = json.loads(args.manifest.read_text())
        for a in ds["articles"]:
            upsert("Articles", a["article_id"], {"article_id": a["article_id"],
                "author": {"wang_xiaobo": "王小波", "wang_zengqi": "汪曾祺"}[a["author_id"]],
                "title": a["title"], "topic": {"reading":"读书与表达", "daily":"日常与物件", "society":"人物与社会"}[a["topic"]],
                "split": {"discovery":"研究样本", "reference":"开发参照", "confirmation":"最终留出"}[a["split"]],
                "characters": a["characters"], "source_url": a["source_url"]})
        print("articles synced", len(ds["articles"]), flush=True)
    events = ROOT / "lark/journal-records.json"
    if events.exists():
        for row in json.loads(events.read_text()):
            upsert("Journal", row["event_id"], row)
    runs = args.runs + ([args.run] if args.run else [])
    for run in runs:
        summary = json.loads((run / "summary.json").read_text())
        rid = run.name
        report_name = {"p1-detailed-pilot-01": "p1-report-doc.json", "p2-detailed-single-01": "p2-single-report-doc.json",
                       "p2-pairs-02": "p2-stop-20261004.json", "p2-routing-flash-01": "p2-stop-20261004.json"}.get(rid)
        report_file = ROOT / "lark" / report_name if report_name else None
        if rid.startswith("p2-candidate"):
            report_file = ROOT / "lark/candidate-progress-20261011.json"
        if rid.startswith("p3-independent"):
            report_file = ROOT / "lark/p3-progress-20261018.json"
        if rid.startswith("p3-evidence-audit"):
            report_file = ROOT / "lark/p3-evidence-audit-20261019.json"
        if rid.startswith("p3-ending-anchor"):
            report_file = ROOT / "lark/ending-anchor-20261020.json"
        report = json.loads(report_file.read_text()) if report_file and report_file.exists() else {}
        report_url = report.get("response", report).get("data", {}).get("doc_url")
        stage = "P1 单篇试跑" if rid.startswith("p1-") else "P2 单篇重复提取" if "repeat" in rid else "P2 单篇提取"
        if rid.startswith("p2-candidate"):
            stage = "P2 候选比较方法验证"
        if rid.startswith("p2-atomic"):
            stage = "P2 原子特征整理（尚未比较）" if summary.get("stage") == "atomic_views" else "P2 原子特征小样本比较"
        if rid.startswith("p3-independent"):
            stage = "P3 独立原文证据聚合小试"
        if rid.startswith("p3-evidence-audit"):
            stage = "P3 固定候选逐篇证据核验"
        if rid.startswith("p3-ending-anchor"):
            stage = "P3 程序固定结尾位置核验"
        for marker, label in [("routing", "Flash路由小样本验证"), ("views", "P2 逐维规则整理"), ("pairs", "P2 成对比较"), ("profiles", "P3 作者规则聚合"), ("convergence", "P4 样本量收敛"), ("generation", "P5 生成对照"), ("reference", "P5 开发评估"), ("confirmation", "P5 确认评估")]:
            if marker in rid:
                stage = label
                break
        rows = summary.get("rows", [])
        checks = ""
        control_file = run / "controls.json"
        control_rows = json.loads(control_file.read_text()).get("rows", []) if control_file.exists() else []
        if control_rows:
            checks = f" 已执行控制题通过 {sum(r.get('passed') is True for r in control_rows)}/{len(control_rows)}（未执行项不计入分母）。"
        elif any("passed" in r for r in rows):
            checks = f" 开发预期一致 {sum(r.get('passed') is True for r in rows)}/{len(rows)}，不等于人评。"
        review_path = run / "semantic-review.json"
        if review_path.exists():
            review = json.loads(review_path.read_text())
            if review.get("decision") == "reject_for_full_matrix":
                checks += " 语义复核未通过，不扩大正式矩阵；工程完成不等于方法采用。"
            elif review.get("decision", "").startswith("reject"):
                checks += " 复核拒绝采用；详见阶段报告，完成数量不表示质量通过。"
        upsert("Runs", rid, {"run_id": rid, "stage": stage, "status": summary["status"],
            "question": f"{stage}：当前步骤是否形成完整可追溯结果？",
            "conclusion": f"请求/产物状态：完成 {sum(r['status'] == 'complete' for r in rows)} 项，失败 {sum(r['status'] == 'failed' for r in rows)} 项，暂缓 {sum(r['status'] == 'deferred' for r in rows)} 项。" + checks + summary.get("error", ""),
            "caveat": "这是最近一次同步快照，running不保证进程仍存活。费用含未知预留；完成数量和字数都不是质量评分。",
            "spent_cny": summary["usage"]["run_occupied_cny"], "report_url": report_url})
        for row in rows if (run / "single").is_dir() else []:
            ident = row.get("artifact_id", row["article_id"])
            spec_path = run / "single" / f"{ident}.md"
            upsert("Styles", f"{rid}/{ident}", {"artifact_id": f"{rid}/{ident}", "run_id": rid,
                "article_id": row["article_id"], "status": row["status"], "characters": row.get("characters"),
                "warnings": "；".join(row.get("warnings", [])), "spec": spec_path.read_text() if spec_path.exists() else ""})
    if "overview" not in state["dashboards"]:
        data = call("+dashboard-create", name="研究进展与样本概况")
        state["dashboards"]["overview"] = data
        save(state)
    (ROOT / "lark/last-sync.json").write_text(json.dumps({"status":"complete", "base":base,
        "manifest":str(args.manifest), "run":str(args.run), "runs":[str(r) for r in runs], "record_count":len(state["records"])}, indent=2))
    print("sync complete", state["url"], flush=True)


if __name__ == "__main__":
    main()
