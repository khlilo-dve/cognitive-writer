#!/usr/bin/env python3
"""Derive all rules from two rejected model receipts for an exploratory writer run."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"
SOURCE = ROOT / "runs/pragmatic-profiles-01"
TARGET = ROOT / "runs/pragmatic-profiles-recovered-01"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    original = json.loads((SOURCE / "summary.json").read_text())
    assert original["status"] == "partial" and len(original["rows"]) == 2
    plans = json.loads((SOURCE / "plans.json").read_text())
    inputs = json.loads((SOURCE / "inputs.json").read_text())
    assert len(plans) == 2
    source_map = inputs["sources"]
    articles = []
    diagnostics = []
    receipts = []
    for plan in plans:
        path = SOURCE / "requests" / (plan["id"] + "-synthesis-1.json")
        receipt_bytes = path.read_bytes()
        checkpoint = json.loads(receipt_bytes)
        serialized = json.dumps(checkpoint["receipt"], ensure_ascii=False, separators=(",", ":")).encode()
        assert digest(serialized) == checkpoint["receipt_sha256"]
        assert checkpoint["receipt"]["model"] == "mimo-v2.6-pro"
        raw = json.loads(checkpoint["receipt"]["content"])
        rules = raw["rules"]
        assert len(rules) == 6 and len({r["mechanism"] for r in rules}) == 6
        by_alias = {f"a{i+1:02}": source_map[article][1] for i, article in enumerate(plan["articles"])}
        md = "# 写作声音执行说明\n\n以下是从多篇文章归纳的探索性表达倾向。先满足当前任务的素材和事实约束。\n\n"
        for index, rule in enumerate(rules, 1):
            for key in ("mechanism", "when", "instruction"):
                assert isinstance(rule[key], str) and rule[key].strip()
            md += f"## {rule['mechanism']}\n\n适用时机：{rule['when']}。\n\n{rule['instruction']}\n\n"
            evidence = rule.get("evidence", [])
            assert 2 <= len(evidence) <= 4
            found, topics = [], set()
            for e in evidence:
                alias, quote = e["article"], e["quote"]
                assert alias in by_alias and isinstance(quote, str)
                data = by_alias[alias]
                exact = 8 <= len(quote) <= 120 and quote in data["text"]
                found.append(dict(article=alias, exact=exact, quote_sha256=digest(quote.encode()), characters=len(quote)))
                topics.add(data["topic"])
            diagnostics.append(dict(author=plan["author"], rule_index=index, mechanism=rule["mechanism"],
                                    topic_count=len(topics), evidence=found))
        characters = len("".join(md.split()))
        warnings = ["探索性派生：原严格聚合run仍为partial，引用与题材限制见独立诊断；不证明稳定作者风格"]
        if characters < 1500:
            warnings.append(f"执行说明{characters}非空白字符，低于1500完整度参考；按原样保留")
        article = dict(plan=plan, profile=None, view=dict(features=[], uncertainties=["尚无可用语义比较"]),
                       spec=md, source_sha256=digest(json.dumps([by_alias[f"a{i+1:02}"] for i in range(10)], ensure_ascii=False, sort_keys=True).encode()),
                       warnings=warnings)
        articles.append(article)
        receipts.append(dict(author=plan["author"], source_receipt_sha256=digest(receipt_bytes)))
    TARGET.mkdir(parents=True, exist_ok=True)
    (TARGET / "profiles").mkdir(exist_ok=True)
    frozen = dict(suite="pragmatic_profile_salvage_v1", source_run=SOURCE.name, source_inputs_sha256=digest((SOURCE / "inputs.json").read_bytes()),
                  protocol_sha256=digest((ROOT / "method-revision-12.md").read_bytes()), plans=plans, source_receipts=receipts,
                  derivation="all six parsed mechanisms per author retained verbatim; source quotes omitted from writer Spec")
    path = TARGET / "inputs.json"
    if path.exists():
        assert json.loads(path.read_text()) == frozen, "Derived run identity changed; use new ID"
    path.write_text(json.dumps(frozen, ensure_ascii=False, indent=2) + "\n")
    (TARGET / "plans.json").write_text(json.dumps(plans, ensure_ascii=False, indent=2) + "\n")
    for article in articles:
        id = article["plan"]["id"]
        (TARGET / "profiles" / (id + ".json")).write_text(json.dumps(article, ensure_ascii=False, indent=2) + "\n")
        (TARGET / "profiles" / (id + ".md")).write_text(article["spec"])
    report = dict(run=TARGET.name, source_run=SOURCE.name, source_status="partial", artifacts=len(articles),
                  rules=12, model_calls=0, diagnostics=diagnostics,
                  caveat="Quotes are not used in the writer Spec; exact-match failures and topic gaps remain explicit, not repaired")
    (TARGET / "evidence-diagnostic.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    # Use the source run's frozen completion snapshot, not a later global ledger.
    occupied = original["usage"]["occupied_cny"]
    rows = [dict(id=a["plan"]["id"], author=a["plan"]["author"], status="complete", rules=6,
                 characters=len("".join(a["spec"].split())), warnings=a["warnings"]) for a in articles]
    (TARGET / "summary.json").write_text(json.dumps(dict(status="complete",stage="profiles",rows=rows,
        source_status="partial",semantic_status="exploratory_only",usage=dict(run_occupied_cny=0,occupied_cny=round(occupied,6),pricing="offline derivation; original calls charged to source run")), ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"source": original["status"], "derived": len(articles), "rules": len(diagnostics),
                      "unmatched_quotes": sum(not e["exact"] for d in diagnostics for e in d["evidence"]),
                      "one_topic_rules": sum(d["topic_count"] < 2 for d in diagnostics)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
