#!/usr/bin/env python3
"""Deterministic discovery-only source statistics; no reference/confirmation text access."""
import csv
import hashlib
import json
import re
import statistics
from pathlib import Path

ROOT = Path("research/style-aggregation")


def main():
    manifest = ROOT / "datasets/authors-v1/manifest.json"
    dataset = json.loads(manifest.read_text())
    rows = []
    for article in dataset["articles"]:
        if article["split"] != "discovery":
            continue
        raw = (manifest.parent / article["path"]).read_bytes()
        if hashlib.sha256(raw).hexdigest() != article["sha256"]:
            raise ValueError(f"Source hash changed: {article['article_id']}")
        text = raw.decode("utf-8")
        count = lambda s: len(re.sub(r"\s", "", s))
        paragraphs = [count(p) for p in re.split(r"\n\s*\n", text) if count(p)]
        sentences = [count(s) for s in re.split(r"[。！？!?]+", text) if count(s)]
        n = count(text)
        row = {k: article[k] for k in ("article_id", "author_id", "title", "topic")}
        row.update(characters=n, paragraphs=len(paragraphs), sentences=len(sentences),
                   paragraph_median=statistics.median(paragraphs),
                   sentence_median=statistics.median(sentences))
        for label, marks in {"question": "？?", "exclamation": "！!", "quote_open": "“「", "first_person": "我", "second_person": "你"}.items():
            total = sum(text.count(mark) for mark in marks)
            row[label + "_per_1000"] = round(total * 1000 / n, 4)
        rows.append(row)
    assert len(rows) == 20
    out = ROOT / "reports/source-description-v1"
    out.mkdir(parents=True, exist_ok=True)
    with (out / "articles.csv").open("w", newline="", encoding="utf-8-sig") as f:
        writer = csv.DictWriter(f, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(rows)
    (out / "provenance.json").write_text(json.dumps({
        "manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "articles": 20, "split": "discovery",
        "limitations": ["标点切句，非语言学句法分析", "人称为字符计数，包含引语和复数", "题材和文本长度可影响统计，不能直接作为作者风格真值", "未读取reference或confirmation正文"]
    }, ensure_ascii=False, indent=2) + "\n")
    print(f"Wrote {len(rows)} verified discovery rows to {out}")


if __name__ == "__main__":
    main()
