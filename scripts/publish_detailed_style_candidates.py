#!/usr/bin/env python3
"""Publish reviewable snapshots of the two detailed writer-facing drafts."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT = Path("research/style-aggregation")
FILES = [
    ("wang-xiaobo", "王小波", "先找值得推演的矛盾"),
    ("wang-zengqi", "汪曾祺", "随题材改变组织方式"),
]


def main():
    for slug, author, marker in FILES:
        path = ROOT / "runs/detailed-executable-style-01/profiles" / f"{slug}.md"
        content = ("开发候选，2026-10-23：已核查十篇发现集原文并写出详细说明，"
                   "尚未经过独立审阅或有效新题材试写，不是正式生产风格。\n\n" + path.read_text())
        doc = publish(f"detailed-style-{slug}-v1", f"Cognitive Writer｜{author}详细风格候选（待审）", content)
        fetched = call("+fetch", doc=doc["data"]["doc_id"])
        rendered = json.dumps(fetched, ensure_ascii=False)
        assert "开发候选" in rendered and marker in rendered
        (ROOT / "lark" / f"detailed-style-{slug}-v1-fetch.json").write_text(
            json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
        print(author, doc["data"]["doc_url"])
    evidence = (ROOT / "runs/detailed-executable-style-01/evidence-review.md").read_text()
    doc = publish("detailed-style-evidence-v1", "Cognitive Writer｜详细风格候选：原文依据与待审问题", evidence)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    rendered = json.dumps(fetched, ensure_ascii=False)
    assert "原文依据与保留问题" in rendered and "标题如何命名" in rendered
    (ROOT / "lark/detailed-style-evidence-v1-fetch.json").write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    print("原文依据", doc["data"]["doc_url"])


if __name__ == "__main__":
    main()
