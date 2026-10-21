#!/usr/bin/env python3
"""Publish immutable P4/P5 development snapshots and verify chart display."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT=Path("research/style-aggregation")


def one(key,title,report,chart,caption,probe):
    content=(ROOT/report).read_text().split("\n",1)[1]
    result=publish(key,title,content)
    doc=result["data"]["doc_id"]
    image_path=ROOT/"lark"/(key+"-image.json")
    if not image_path.exists():
        response=call("+media-insert",doc=doc,file=ROOT/chart,caption=caption)
        image_path.write_text(json.dumps(response,ensure_ascii=False,indent=2)+"\n")
    fetched=call("+fetch",doc=doc)
    encoded=json.dumps(fetched,ensure_ascii=False)
    assert probe in encoded and "<image" in encoded, key
    (ROOT/"lark"/(key+"-fetch.json")).write_text(json.dumps(fetched,ensure_ascii=False,indent=2)+"\n")
    print(result["data"]["doc_url"])


def main():
    one("surface-convergence-20261021","Cognitive Writer｜20篇原文的样本量对照",
        "reports/surface-convergence-v1/report.md","charts/surface-convergence-v1/surface-convergence.png",
        "冻结10篇原文内的句长、段长等可观察量；10篇距离为零是定义，不表示作者风格已稳定。", "10篇差距")
    one("pragmatic-generation-20261021","Cognitive Writer｜60篇探索性写作对照进展",
        "reports/pragmatic-generation-01/report.md","charts/pragmatic-generation-01/length-compliance.png",
        "两位作者的聚合、单篇、无风格生成字数达标率；长度不是风格或事实质量分。", "60篇")


if __name__=="__main__":main()
