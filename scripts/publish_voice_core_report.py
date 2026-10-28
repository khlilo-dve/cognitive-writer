#!/usr/bin/env python3
"""Publish the verified core milestone and explicitly marked ten-article style drafts."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from publish_style_progress import call
from publish_style_stop_report import publish

ROOT = Path("research/style-aggregation")


def verified(key, title, text, phrase):
    doc = publish(key, title, text)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    if phrase not in json.dumps(fetched, ensure_ascii=False):
        raise ValueError("Published snapshot did not contain expected disclosure")
    (ROOT / "lark" / f"{key}-fetch.json").write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n"
    )
    return doc["data"]["doc_url"]


def main():
    links = []
    for code, author, issue in (
        ("wxb", "王小波", "仍有至少一次换算等配额式建议，不能直接当作固定规则。"),
        ("wzq", "汪曾祺", "把具体到判断写成单向通道过硬，原文也存在先判断后佐证。"),
    ):
        spec = (ROOT / f"runs/voice-learning-core-01/profiles/{code}-k10.md").read_text()
        preface = ("本文件是**待审草稿**，来自当前十篇发现集。核心流程和来源身份已核对，"
                   "尚未独立验收全部语义，也未证明新题材Writer事实可靠。" + issue + "\n\n---\n\n")
        url = verified(f"voice-core-{code}-k10-20261028", f"Cognitive Writer｜{author}十篇声音说明（待审草稿）",
                       preface + spec, "待审草稿")
        links.append(f"- [{author}十篇说明（待审草稿）]({url})")
    report = (ROOT / "reports/learning-core-stage-2026-10-28.md").read_text()
    report += "\n\n## 可直接阅读的实际产物\n\n" + "\n".join(links) + "\n"
    url = verified("learning-core-stage-20261028", "Cognitive Writer｜1–10篇正文学习核心阶段报告",
                   report, "21.133481")
    print(json.dumps({"report_url": url, "drafts": links, "verified": True}, ensure_ascii=False))


if __name__ == "__main__":
    main()
