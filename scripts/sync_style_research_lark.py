#!/usr/bin/env python3
"""Idempotent, sequential CLI projection of local research artifacts into the user's Base."""
import argparse
import json
import re
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
    ap.add_argument("--journal-from", help="Sync journal events from this inclusive E0000 ID")
    args = ap.parse_args()
    if args.journal_from and not re.fullmatch(r"E[0-9]{4}", args.journal_from):
        ap.error("--journal-from requires an E0000 event ID")
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
            if args.journal_from and row["event_id"] < args.journal_from:
                continue
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
        if rid.startswith("detailed-transfer"):
            report_file = ROOT / "lark/detailed-style-stage-a-20261023.json"
        if rid.startswith("voice-learning-"):
            report_file = ROOT / "lark/learning-core-stage-20261028.json"
        if rid.startswith("voice-learning-scope"):
            report_file = ROOT / "lark/voice-scope-stage-20261029.json"
        if rid.startswith("learning-workspace"):
            report_file = ROOT / "lark/learning-workspace-stage-20261031.json"
        if rid.startswith("learning-jobs"):
            report_file = ROOT / "lark/learning-jobs-stage-20261102.json"
        if rid.startswith(("learning-jobs-quality", "learning-quality-")):
            report_file = ROOT / "lark/learning-quality-stage-20261102.json"
        if rid in ("learning-jobs-quality-01", "learning-quality-probe-01"):
            report_file = ROOT / "lark/learning-quality-recovery-stage-20261102.json"
        if rid.startswith("learning-input-"):
            report_file = ROOT / "lark/learning-input-20261103.json"
        if rid.startswith("learning-agent-"):
            report_file = ROOT / "lark/learning-agent-20261103.json"
        grounded_prefixes = ("learning-qualifier-", "learning-grounded-", "learning-boundary-")
        if rid.startswith(grounded_prefixes):
            report_file = ROOT / "lark/learning-grounded-stage-20261102.json"
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
        if rid.startswith("detailed-transfer"):
            stage = "详细Style新题材试写"
        voice_stages = {
            "voice-learning-scope-reviewed": "原文复读｜实现者有限定点修订",
            "voice-learning-scope-recovered": "原文复读｜Markdown包装零请求恢复",
            "voice-learning-scope-probe": "范围修订｜同条件前后与中性试写",
            "voice-learning-scope": "原文复读与范围修订",
            "voice-learning-core": "通用正文学习核心｜1/3/10篇",
            "voice-learning-clean-pro": "Pro执行说明内容编译",
            "voice-learning-clean": "Flash内容剥离小试",
            "voice-learning-probe": "单篇与多篇新题材执行诊断",
            "voice-learning-prose": "完整原文长篇声音分析",
            "voice-learning-editor": "上位声音编辑小试",
            "voice-learning-vertical": "两篇增量学习竖切",
            "voice-learning-incremental": "逐篇卡片与增量综合小试",
            "voice-learning-pilot": "单轮综合1/2/3篇小试",
        }
        for prefix, label in voice_stages.items():
            if rid.startswith(prefix):
                stage = label
                break
        if rid.startswith("learning-workspace"):
            stage = "风格草稿工作区｜预览、版本与导出本地验证"
        if rid.startswith("learning-jobs"):
            stage = "正文学习任务｜真实1→2篇链与恢复"
        if rid.startswith("learning-jobs-quality"):
            stage = "学习质量改进｜恢复后四任务核查" if summary["status"] == "complete" else "学习质量改进｜真实调用传输中断"
        if rid.startswith("learning-quality-reviewed"):
            stage = "来源范围修订｜实现者派生阅读稿"
        if rid.startswith("learning-quality-probe"):
            stage = "新题材执行检查｜10稿完成与事实核查" if summary["status"] == "complete" else "新题材执行检查｜依赖未完成暂缓"
        for marker, label in [("routing", "Flash路由小样本验证"), ("views", "P2 逐维规则整理"), ("pairs", "P2 成对比较"), ("profiles", "P3 作者规则聚合"), ("convergence", "P4 样本量收敛"), ("generation", "P5 生成对照"), ("reference", "P5 开发评估"), ("confirmation", "P5 确认评估")]:
            if marker in rid:
                stage = label
                break
        grounded_labels = {
            "learning-qualifier-01": "限定语修订｜内容隔离失败",
            "learning-qualifier-isolated-01": "分析复用｜因错文引用中断",
            "learning-grounded-01": "引文归属｜两完成两失败",
            "learning-grounded-repair-01": "引文归属｜定位修复确认",
            "learning-boundary-probe-01": "定点执行｜感受与抽象材料四稿",
        }
        stage = grounded_labels.get(rid, stage)
        if rid.startswith("learning-input-"):
            stage = "产品输入工具｜采集、冻结与导出本地验证"
        if rid.startswith("learning-agent-"):
            stage = "P3材料领域Agent｜有限决策与恢复"
        rows = summary.get("rows", [])
        checks = ""
        control_file = run / "controls.json"
        control_rows = json.loads(control_file.read_text()).get("rows", []) if control_file.exists() else []
        if control_rows:
            checks = f" 已执行控制题通过 {sum(r.get('passed') is True for r in control_rows)}/{len(control_rows)}（未执行项不计入分母）。"
        elif any("passed" in r for r in rows):
            checks = f" 开发预期一致 {sum(r.get('passed') is True for r in rows)}/{len(rows)}，不等于人评。"
        review_path = run / "semantic-review.json"
        recovered_review = run / "semantic-review-recovered.json"
        if recovered_review.exists():
            review_path = recovered_review
        if review_path.exists():
            review = json.loads(review_path.read_text())
            if review.get("decision") == "reject_for_full_matrix":
                checks += " 语义复核未通过，不扩大正式矩阵；工程完成不等于方法采用。"
            elif review.get("decision", "").startswith("reject"):
                checks += " 复核拒绝采用；详见阶段报告，完成数量不表示质量通过。"
            elif review.get("decision", "").startswith("development_core_workflow_verified"):
                checks += " 核心流程与来源/复跑已验证，Style仍是待审草稿，语义与Writer事实未全部通过。"
            elif review.get("decision"):
                checks += " 仅开发候选或探索性诊断，尚未产品采用；具体边界见阶段报告。"
        if rid.startswith(("learning-jobs", "learning-qualifier", "learning-grounded")) and rows and all("jobs" in r for r in rows):
            jobs = [job for row in rows for job in row["jobs"]]
            checks += f" {len(rows)}条作者链内共{len(jobs)}个已记录的学习任务，完成{sum(j.get('status') == 'complete' for j in jobs)}个；保存仅在研究工作区，Style仍待审。"
        if rid.startswith("learning-quality-reviewed"):
            checks += " 四份是旧模型稿上的21处实现者派生修改，0模型请求，不代表新自动方法产物或Writer验收。"
        if rid == "learning-quality-probe-01" and (run / "qualitative-readout.json").exists():
            checks += " 已逐稿读10篇并记录具体事实问题，中性稿也有越界；不是可发布成稿或质量通过。"
        conclusion = f"请求/产物状态：完成 {sum(r['status'] == 'complete' for r in rows)} 项，失败 {sum(r['status'] == 'failed' for r in rows)} 项，暂缓 {sum(r['status'] == 'deferred' for r in rows)} 项，中断 {sum(r['status'] == 'interrupted' for r in rows)} 项。" + checks + summary.get("error", "")
        if rid.startswith("learning-agent-"):
            conclusion = f"材料就绪 {sum(r['status'] == 'ready_for_learning' for r in rows)}，明确受阻 {sum(r['status'] == 'blocked' for r in rows)}，执行失败 {sum(r['status'] == 'failed' for r in rows)}。" + checks
            if rid.endswith("local-01"):
                conclusion = f"本地协议场景 {len(rows)} 组通过；14次本地模拟请求、0付费请求。"
        upsert("Runs", rid, {"run_id": rid, "stage": stage, "status": summary["status"],
            "question": f"{stage}：当前步骤是否形成完整可追溯结果？",
            "conclusion": conclusion,
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
        "manifest":str(args.manifest), "run":str(args.run), "runs":[str(r) for r in runs],
        "journal_from":args.journal_from, "record_count":len(state["records"])}, indent=2))
    print("sync complete", state["url"], flush=True)


if __name__ == "__main__":
    main()
