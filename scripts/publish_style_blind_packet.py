#!/usr/bin/env python3
"""Publish the frozen anonymous packet as a user-owned Feishu Doc and read it back."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT = Path("research/style-aggregation")


def main():
    packet = (ROOT / "blind-test/ab-v1/packet.md").read_text()
    doc = publish("blind-ab-v1-packet", "Cognitive Writer｜匿名写作 A/B 阅读材料", packet)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    rendered = json.dumps(fetched, ensure_ascii=False)
    for marker in ("作者一原文短片段", "作者二原文短片段", "第 一组", "第 二组", "第 三组", "第 四组"):
        if marker.startswith("第 "):
            marker = marker.replace("第 ", "第")
        assert marker in rendered, marker
    assert "王小波" not in rendered and "汪曾祺" not in rendered
    (ROOT / "lark/blind-ab-v1-packet-fetch.json").write_text(json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    print(doc["data"]["doc_url"])


if __name__ == "__main__":
    main()
