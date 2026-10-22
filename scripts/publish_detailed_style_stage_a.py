#!/usr/bin/env python3
"""Publish the readable stage-A report after the candidate Docs are verified."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT = Path("research/style-aggregation")


def main():
    content = (ROOT / "reports/detailed-style-stage-a-2026-10-23.md").read_text()
    doc = publish("detailed-style-stage-a-20261023", "Cognitive Writer｜详细风格文件：两份候选与试写状态", content)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    rendered = json.dumps(fetched, ensure_ascii=False)
    for marker in ("两份候选可阅读", "一篇新稿也没有", "19.624939"):
        assert marker in rendered, marker
    (ROOT / "lark/detailed-style-stage-a-20261023-fetch.json").write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    print(doc["data"]["doc_url"])


if __name__ == "__main__":
    main()
