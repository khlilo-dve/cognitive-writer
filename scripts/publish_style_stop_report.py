#!/usr/bin/env python3
"""Publish the immutable diagnosis and reviewable next-method proposal to user-owned Docs."""
import hashlib
import json
import re
import sys
from pathlib import Path
sys.dont_write_bytecode=True
from publish_style_progress import call

ROOT=Path("research/style-aggregation")


def publish(key,title,content):
    path=ROOT/"lark"/(key+".json")
    digest=hashlib.sha256(content.encode()).hexdigest()
    if path.exists():
        saved=json.loads(path.read_text())
        if saved["source_sha256"]!=digest:raise ValueError("Published snapshot changed; preserve historical document")
        return saved["response"]
    response=call("+create",title=title,markdown=content)
    path.write_text(json.dumps(dict(source_sha256=digest,response=response),ensure_ascii=False,indent=2)+"\n")
    return response


def main():
    proposal=(ROOT/"P2-comparator-proposal-2026-10-04.md").read_text().split("\n",1)[1]
    proposal=publish("p2-proposal-20261004","Cognitive Writer｜比较器修正提案（待认可）",proposal)
    proposal_url=proposal["data"]["doc_url"]
    report=(ROOT/"reports/P2-measurement-stop-2026-10-04.md").read_text().split("\n",1)[1]
    report=re.sub(r"!\[[^\]]*\]\([^)]*\)","（结构问题图表附在文末）",report)
    report=report.replace("../P2-comparator-proposal-2026-10-04.md",proposal_url)
    report=report.replace("[离线诊断](P2-comparison-diagnosis/candidate-diagnostic.json)","项目研究目录中的candidate-diagnostic.json")
    doc=publish("p2-stop-20261004","Cognitive Writer｜阶段报告：比较环节的失败与下一步",report)
    image_state=ROOT/"lark/p2-stop-20261004-image.json"
    if not image_state.exists():
        media=call("+media-insert",doc=doc["data"]["doc_id"],file=ROOT/"charts/p2-comparison-stop/structure-issues.png",caption="52份Pro首轮响应的结构诊断；两类问题可以重叠，不是作者风格质量分。")
        image_state.write_text(json.dumps(media,ensure_ascii=False,indent=2)+"\n")
    for name,result,expected in [("proposal",proposal,"最多24"),("report",doc,"7.382993")]:
        fetched=call("+fetch",doc=result["data"]["doc_id"])
        encoded=json.dumps(fetched,ensure_ascii=False)
        assert expected in encoded
        if name=="report":assert "<image" in encoded and proposal_url in encoded
        (ROOT/"lark"/f"p2-stop-20261004-{name}-fetch.json").write_text(json.dumps(fetched,ensure_ascii=False,indent=2)+"\n")
    print(json.dumps(dict(report_url=doc["data"]["doc_url"],proposal_url=proposal_url,verified=True),ensure_ascii=False))


if __name__=="__main__":main()
