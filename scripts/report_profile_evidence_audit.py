#!/usr/bin/env python3
"""Summarize independent evidence votes without treating model labels as truth."""
import argparse
import csv
import hashlib
import json
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    args = ap.parse_args()
    assert Path(args.run).name == args.run
    root = ROOT / "runs" / args.run
    frozen = json.loads((root / "inputs.json").read_text())
    summary = json.loads((root / "summary.json").read_text())
    assert summary["status"] in ("complete", "partial")
    controls = json.loads((root / "controls.json").read_text())
    assert controls["passed"] and len(controls["rows"]) == 4
    receipts = []
    for p in sorted((root / "requests").glob("*.json")):
        c = json.loads(p.read_text())
        assert hashlib.sha256(json.dumps(c["receipt"], ensure_ascii=False, separators=(",", ":")).encode()).hexdigest() == c["receipt_sha256"]
        receipts.append({"file": p.name, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()})
    aggregate_path = root / "aggregates.json"
    if aggregate_path.exists():
        aggregates = json.loads(aggregate_path.read_text())
    else:
        aggregates = []
        for author, candidates in frozen["candidates"].items():
            rules = []
            for candidate in candidates:
                evidence = []
                for row in summary["rows"]:
                    if row["status"] != "complete" or row["author"] != author:
                        continue
                    profile = json.loads((root / "articles" / (row["id"]+".json")).read_text())
                    found = next(r["candidate"] for r in profile["rules"] if r["candidate"]["id"] == candidate["id"])
                    assert all(found[k] == candidate[k] for k in ("id", "dimension", "rule", "condition"))
                    evidence += found["evidence"]
                rules.append(dict(candidate={**candidate, "evidence": evidence}, classification="not_classified_incomplete",
                                  **{status+"_articles": sorted(e["article"] for e in evidence if e["status"] == status) for status in ("support", "counter", "unknown")}))
            aggregates.append(dict(author=author, profile=dict(rules=rules)))
    rows = []
    previous = ROOT / "runs/p3-independent-pilot-02"
    previous_plans = json.loads((previous / "plans.json").read_text())
    review = ["# 固定候选逐篇核验材料", "", "程序审计不代替语义复核；以下支持票是模型判断。", ""]
    for aggregate in aggregates:
        author = aggregate["author"]
        old_plan = next(p for p in previous_plans if p["author"] == author)
        old_cp = json.loads((previous / "requests" / (old_plan["id"]+"-merge-0.json")).read_text())
        old_rules = json.loads(old_cp["receipt"]["content"])["rules"]
        old_ids = {f"a{i+1:02}": article for i, article in enumerate(old_plan["articles"])}
        assert len(aggregate["profile"]["rules"]) == len(frozen["candidates"][author])
        for rule, candidate in zip(aggregate["profile"]["rules"], frozen["candidates"][author]):
            c = rule["candidate"]
            assert all(c[k] == candidate[k] for k in ("id", "dimension", "rule", "condition"))
            expected = {k for k, v in frozen["sources"].items() if v[0] == author}
            observed = {e["article"] for e in c["evidence"]}
            assert observed <= expected and len(observed) == len(c["evidence"])
            missing = sorted(expected - observed)
            votes = Counter(e["status"] for e in c["evidence"])
            old_rule = next(r for r in old_rules if r["id"] == c["id"])
            assert all(old_rule[k] == c[k] for k in ("id", "dimension", "rule", "condition"))
            old_support = sum(e["status"] == "support" and old_ids[e["article"]] in observed for e in old_rule["evidence"])
            review += [f"## {author} / {c['id']} / {rule['classification']}", c["rule"], "条件：" + c["condition"], ""]
            for e in c["evidence"]:
                text = frozen["sources"][e["article"]][1]["text"]
                assert (not e["quote"] and e["status"] == "unknown") or e["quote"] in text
                review += [f"- {e['article']} / {e['status']}：{e['reason']}"]
                if e["quote"]:
                    review += ["", "\n".join("> " + line if line else ">" for line in e["quote"].split("\n")), ""]
            for status in ("support", "counter", "unknown"):
                assert sorted(e["article"] for e in c["evidence"] if e["status"] == status) == rule[status+"_articles"]
            rows.append(dict(author=author, rule=c["id"], old_self_reported_support_same_articles=old_support, support=votes["support"], counter=votes["counter"], unknown=votes["unknown"], missing=len(missing),
                             support_upper_bound=votes["support"]+len(missing), classification=rule["classification"]))
            review += ["缺失文章（不算unknown）："+", ".join(missing), ""]
    out = ROOT / "reports" / args.run
    out.mkdir(parents=True, exist_ok=True)
    with (out / "votes.csv").open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    (out / "review-material.md").write_text("\n".join(review).rstrip()+"\n")
    (out / "audit.json").write_text(json.dumps(dict(receipts=receipts, rows=rows, inputs_sha256=hashlib.sha256((root / "inputs.json").read_bytes()).hexdigest(), limitation="Model evidence votes, not human labels or adoption decision"), ensure_ascii=False, indent=2)+"\n")
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    plt.rcParams.update({"font.family": "Noto Sans SC", "axes.unicode_minus": False})
    fig, ax = plt.subplots(figsize=(10, 7), layout="constrained")
    labels = [("王小波" if r["author"] == "wang_xiaobo" else "汪曾祺") + " / " + r["rule"] for r in rows]
    positions = list(range(len(rows)))
    ax.barh([i-.18 for i in positions], [r["old_self_reported_support_same_articles"] for r in rows], .35, label="旧聚合自报支持（未通过验证）", color="#aab7be")
    ax.barh([i+.18 for i in positions], [r["support"] for r in rows], .35, label="逐篇核验支持（仍是模型判断）", color="#357a91")
    ax.set_yticks(positions, labels)
    ax.invert_yaxis()
    ax.set_xlim(0, 10)
    ax.set_xlabel("同批有效文章上的支持票数；王小波9篇、汪曾祺7篇")
    ax.set_title("固定规则的支持票变化\n不能据此判断哪种模型标注更准确")
    ax.legend(loc="lower right")
    ax.spines[["top", "right"]].set_visible(False)
    chart = ROOT / "charts" / args.run
    chart.mkdir(parents=True, exist_ok=True)
    fig.savefig(chart / "support-votes.png", dpi=170)
    plt.close(fig)
    print(json.dumps(dict(receipts=len(receipts), rows=rows, usage={k:v for k,v in summary["usage"].items() if k!="entries"}), ensure_ascii=False))


if __name__ == "__main__":
    main()
