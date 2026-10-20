#!/usr/bin/env python3
"""Offline profile provenance audit and full semantic review packet; no scoring API."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    args = ap.parse_args()
    assert Path(args.run).name == args.run
    root = ROOT / "runs" / args.run
    frozen = json.loads((root / "inputs.json").read_text())
    sources = frozen["sources"]
    receipts = []
    for p in sorted((root / "requests").glob("*.json")):
        cp = json.loads(p.read_text())
        assert hashlib.sha256(json.dumps(cp["receipt"], ensure_ascii=False, separators=(",", ":")).encode()).hexdigest() == cp["receipt_sha256"]
        receipts.append({"file": p.name, "sha256": digest(p), "finish_reason": cp["receipt"]["finish_reason"]})
    lines = ["# 聚合证据逐条复核材料", "", "程序只验证身份、引用与计数；语义是否支持必须另行审查，不能当人评。", ""]
    profiles = []
    for plan in frozen["plans"]:
        p = root / "profiles" / (plan["id"] + "-evidence.json")
        if not p.exists():
            continue
        profile = json.loads(p.read_text())
        anonymous = {f"a{i+1:02}": sources[a][1] for i, a in enumerate(plan["articles"])}
        if plan["mode"] != "primary":
            raise ValueError("This review tool accepts primary profiles only")
        lines += [f"## {plan['author']} / {plan['id']}", ""]
        for rule in profile["rules"]:
            c = rule["candidate"]
            assert {e["article"] for e in c["evidence"]} == set(anonymous)
            assert len(c["evidence"]) == len(anonymous)
            support, counter, unknown, topics = [], [], [], set()
            lines += [f"### {c['id']} · {rule['classification']} · {c['dimension']}", c["rule"], "条件："+c["condition"], ""]
            for e in c["evidence"]:
                source = anonymous[e["article"]]
                if e["status"] == "unknown":
                    assert e["quote"] == ""
                    unknown.append(source["article_id"])
                else:
                    assert e["quote"] in source["text"] and len(e["quote"]) >= 6
                    (support if e["status"] == "support" else counter).append(source["article_id"])
                for feature in e["feature_ids"]:
                    assert any(f["id"] == feature and f["dimension"] == c["dimension"] for f in source["view"]["features"])
                if e["status"] == "support":
                    assert e["feature_ids"]
                    topics.add(source["topic"])
                lines += [f"- {source['article_id']} / {e['status']}：{e['reason']}"]
                if e["quote"]:
                    lines += ["", "\n".join("> " + line if line else ">" for line in e["quote"].split("\n")), ""]
            assert sorted(support) == rule["support_articles"]
            assert sorted(counter) == rule["counter_articles"]
            assert sorted(unknown) == rule["unknown_articles"]
            assert sorted(topics) == rule["support_topics"]
        final = root / "profiles" / (plan["id"] + ".json")
        if final.exists():
            artifact = json.loads(final.read_text())
            assert artifact["profile"] == profile
            assert artifact["spec"] == final.with_suffix(".md").read_text()
        profiles.append({"id": plan["id"], "author": plan["author"], "rules": len(profile["rules"]), "sha256": digest(p), "compiled": final.exists()})
    out = ROOT / "reports" / args.run
    out.mkdir(parents=True, exist_ok=True)
    result = {"run": args.run, "inputs_sha256": digest(root / "inputs.json"), "receipts": receipts, "profiles": profiles, "semantic_acceptance": "not_assessed_by_this_script"}
    (out / "audit.json").write_text(json.dumps(result, ensure_ascii=False, indent=2)+"\n")
    (out / "review-material.md").write_text("\n".join(lines).rstrip()+"\n")
    print(json.dumps({"receipts": len(receipts), "profiles": profiles}, ensure_ascii=False))


if __name__ == "__main__":
    main()
