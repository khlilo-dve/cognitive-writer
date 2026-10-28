#!/usr/bin/env python3
"""Publish and read back the current style-learning stage report in user-owned Lark Docs."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from publish_style_progress import call
from publish_style_stop_report import publish

ROOT = Path("research/style-aggregation")


def main():
    source = (ROOT / "reports/learning-core-stage-2026-10-26.md").read_text()
    doc = publish("learning-core-stage-20261026", "Cognitive Writer｜风格学习核心阶段报告", source)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    content = json.dumps(fetched, ensure_ascii=False)
    for phrase in ("单篇和多篇", "这还不是可交付", "20.515326", "原文"):
        if phrase not in content:
            raise ValueError(f"Published report did not contain: {phrase}")
    (ROOT / "lark/learning-core-stage-20261026-fetch.json").write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n"
    )
    print(json.dumps({"url": doc["data"]["doc_url"], "verified": True}, ensure_ascii=False))


if __name__ == "__main__":
    main()
