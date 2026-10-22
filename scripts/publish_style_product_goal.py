#!/usr/bin/env python3
"""Publish the current product goal to the user's Lark Docs and verify its contents."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from publish_style_progress import call
from publish_style_stop_report import publish

ROOT = Path("research/style-aggregation")


def main():
    source = (ROOT / "product-goal-2026-10-23.md").read_text()
    doc = publish("product-goal-20261023", "Cognitive Writer｜单篇与多篇稳定风格学习：产品目标", source)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    content = json.dumps(fetched, ensure_ascii=False)
    for expected in ("单篇", "多篇", "S1", "S4", "增量稳定"):
        if expected not in content:
            raise ValueError(f"Published document did not contain: {expected}")
    (ROOT / "lark/product-goal-20261023-fetch.json").write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n"
    )
    print(json.dumps({"url": doc["data"]["doc_url"], "verified": True}, ensure_ascii=False))


if __name__ == "__main__":
    main()
