#!/usr/bin/env python3
"""Publish and verify the immutable user-facing correction report."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT = Path("research/style-aggregation")


def main():
    report = (ROOT / "reports/direction-correction-2026-10-23.md").read_text()
    doc = publish("direction-correction-20261023", "Cognitive Writer｜研究方向纠偏：先把风格写明白", report)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    rendered = json.dumps(fetched, ensure_ascii=False)
    for marker in ("让写作 AI 看懂", "六条机制", "尚未分发", "19.428330"):
        assert marker in rendered, marker
    (ROOT / "lark/direction-correction-20261023-fetch.json").write_text(json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    print(doc["data"]["doc_url"])


if __name__ == "__main__":
    main()
