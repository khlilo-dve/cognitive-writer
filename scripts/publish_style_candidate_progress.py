#!/usr/bin/env python3
"""Publish one immutable research report and verify its image and figures."""
import json
import sys
from pathlib import Path
sys.dont_write_bytecode=True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT=Path("research/style-aggregation")
def main():
    content=(ROOT/"reports/candidate-progress-2026-10-11/report.md").read_text().split("\n",1)[1]
    result=publish("candidate-progress-20261011","Cognitive Writer｜比较方法改进与失败记录（10月11日）",content)
    media_path=ROOT/"lark/candidate-progress-20261011-image.json"
    if not media_path.exists():
        media=call("+media-insert",doc=result["data"]["doc_id"],file=ROOT/"charts/candidate-progress-2026-10-11/costs.png",caption="四轮方法验证的费用，不是模型质量排名；失败费用同样保留。")
        media_path.write_text(json.dumps(media,ensure_ascii=False,indent=2)+"\n")
    fetched=call("+fetch",doc=result["data"]["doc_id"])
    value=json.dumps(fetched,ensure_ascii=False)
    assert "<image" in value and "尚未得出聚合优劣结论" in value
    (ROOT/"lark/candidate-progress-20261011-fetch.json").write_text(json.dumps(fetched,ensure_ascii=False,indent=2)+"\n")
    print(result["data"]["doc_url"])

if __name__=="__main__":main()
