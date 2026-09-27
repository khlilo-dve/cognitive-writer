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
        verified = call("+record-get", table_id=state["tables"][table], record_id=rid)["record"]
        # Base normalizes an empty text cell to null; nonempty text remains exact.
        if any(verified.get(k) != v and not (stored_fields[k] == "text" and v == "" and verified.get(k) is None)
               and not (k in url_fields and v and verified.get(k) == f"[{v}]({v})")
               for k, v in fields.items()):
            raise ValueError(f"Remote record differs after write: {identity}")
        return verified

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
    if args.run:
        summary = json.loads((args.run / "summary.json").read_text())
        rid = args.run.name
        report_file = ROOT / "lark/p1-report-doc.json"
        report_url = json.loads(report_file.read_text())["data"]["doc_url"] if rid == "p1-detailed-pilot-01" and report_file.exists() else None
        upsert("Runs", rid, {"run_id": rid, "stage": "P1 单篇试跑", "status": summary["status"],
            "question": "详细风格能否独立提取、准确计费并断点复用？",
            "conclusion": f"已记录 {len(summary['rows'])} 篇；详见单篇风格表。",
            "caveat": "这是工程试跑，不证明聚合有效，也不是跨模型质量对照。",
            "spent_cny": summary["usage"]["run_occupied_cny"], "report_url": report_url})
        for row in summary["rows"]:
            ident = row["article_id"]
            spec_path = args.run / "single" / f"{ident}.md"
            upsert("Styles", f"{rid}/{ident}", {"artifact_id": f"{rid}/{ident}", "run_id": rid,
                "article_id": ident, "status": row["status"], "characters": row.get("characters"),
                "warnings": "；".join(row.get("warnings", [])), "spec": spec_path.read_text() if spec_path.exists() else ""})
    if "overview" not in state["dashboards"]:
        data = call("+dashboard-create", name="研究进展与样本概况")
        state["dashboards"]["overview"] = data
        save(state)
    (ROOT / "lark/last-sync.json").write_text(json.dumps({"status":"complete", "base":base,
        "manifest":str(args.manifest), "run":str(args.run), "record_count":len(state["records"])}, indent=2))
    print("sync complete", state["url"], flush=True)


if __name__ == "__main__":
    main()
