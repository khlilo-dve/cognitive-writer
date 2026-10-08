#!/usr/bin/env python3
"""Audit saved candidate judgments and print review material; never calls a model."""
import argparse
import collections
import hashlib
import json
from pathlib import Path

ROOT=Path("research/style-aggregation")

def main():
    ap=argparse.ArgumentParser();ap.add_argument("run");args=ap.parse_args()
    root=ROOT/"runs"/args.run
    receipts=[]
    for p in sorted((root/"requests").glob("*.json")):
        cp=json.loads(p.read_text());r=cp["receipt"]
        assert hashlib.sha256(json.dumps(r,ensure_ascii=False,separators=(",",":")).encode()).hexdigest()==cp["receipt_sha256"]
        receipts.append(dict(name=p.stem,usage=r["usage"],finish_reason=r["finish_reason"],sha256=hashlib.sha256(p.read_bytes()).hexdigest()))
    pairs=[];review=[]
    for p in sorted((root/"pairs").glob("*.json")):
        v=json.loads(p.read_text());cs={c["candidate_id"]:c for c in v["candidates"]};js=v["judgments"]
        assert len(cs)==len(js)==len({j["candidate_id"] for j in js})
        assert set(cs)=={j["candidate_id"] for j in js}
        assert all(c["left"]["dimension"]==c["right"]["dimension"] for c in cs.values())
        selected=v["metrics"]["selected_equivalent_edges"]
        assert len(selected)==len({l for l,r in selected})==len({r for l,r in selected})
        pairs.append(dict(id=v["id"],left=v["left"],right=v["right"],candidates=len(cs),labels=dict(collections.Counter(j["label"] for j in js)),selected=len(selected)))
        # Every equivalent/opposite/uncertain judgment is reviewed, plus deterministic distinct samples.
        for i,j in enumerate(sorted(js,key=lambda x:x["candidate_id"])):
            if j["label"]!="distinct" or i%12==0:
                c=cs[j["candidate_id"]]
                review.append(dict(pair=v["id"],candidate=j["candidate_id"],label=j["label"],dimension=c["left"]["dimension"],
                    left=c["left"]["rule"],left_condition=c["left"]["condition"],right=c["right"]["rule"],right_condition=c["right"]["condition"],reason=j["reason"]))
    result=dict(run=args.run,receipts=receipts,pairs=pairs,review=review,
        limitation="开发数据的实现者复核，不是人工盲测或独立确认；结构有效不等于语义正确")
    out=ROOT/"reports"/args.run;out.mkdir(parents=True,exist_ok=True)
    (out/"audit.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
    lines=["# 候选语义复核材料",result["limitation"],""]
    for r in review:
        lines += [f"## {r['pair']} / {r['candidate']} / {r['label']} / {r['dimension']}",
                  f"A：{r['left']}（条件：{r['left_condition']}）",f"B：{r['right']}（条件：{r['right_condition']}）",f"模型理由：{r['reason']}",""]
    (out/"review-material.md").write_text("\n".join(lines)+"\n")
    print(json.dumps(dict(receipts=len(receipts),pairs=pairs,review_items=len(review)),ensure_ascii=False))

if __name__=="__main__":main()
