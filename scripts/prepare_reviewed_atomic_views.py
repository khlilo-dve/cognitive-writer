#!/usr/bin/env python3
"""Explicit, source-grounded QA corrections; preserve every original model output."""
import hashlib
import json
from pathlib import Path
ROOT=Path("research/style-aggregation")
SOURCE="p2-atomic-pilot-02"
TARGET="p2-atomic-reviewed-01"

def main():
    source=ROOT/"runs"/SOURCE;target=ROOT/"runs"/TARGET
    original=json.loads((source/"inputs.json").read_text())
    revisions=[("wxb-01-r2","f01","condition","未限定","原文全篇用我判断；错误的在场条件来自通用事实许可。按v2预定规则移出voice条件，原文正确的不在场许可保留在排除记录。"),
        ("wzq-02","f19","dimension","opening","该条明确限定篇首或题记，按冻结优先级属于opening。"),
        ("wzq-02","f17","dimension","information","该条仅谈判断后的自我修正，不含事件/引语与评论的接续，依操作定义属于information。"),
        ("wxb-01","f17","dimension","voice","该条谈审视自身的人称与态度，依操作定义属于voice。")]
    raw={}; hashes={}
    for ident in original["sources"]:
        p=source/"views"/(ident+".json");raw[ident]=json.loads(p.read_text());hashes[ident]=hashlib.sha256(p.read_bytes()).hexdigest()
    changes=[]
    for ident,fid,field,value,reason in revisions:
        f=next(f for f in raw[ident]["view"]["features"] if f["id"]==fid)
        if ident=="wxb-01-r2" and fid=="f01":
            assert "只作在场判断" in f["condition"] and "只作不在场的判断" in f["quote"]
            raw[ident]["view"]["uncertainties"].append("校核排除：亲历许可属通用事实政策；无依据时只作不在场判断，不作为作者voice特征的适用条件。")
        changes.append(dict(article=ident,feature=fid,field=field,before=f[field],after=value,reason=reason,quote=f["quote"]))
        f[field]=value
    inputs={**original,"suite":"atomic_view_reviewed_v1","source_run":SOURCE,"raw_view_sha256":hashes,"corrections":changes,
        "review_scope":"实现者按已冻结操作定义校核已定位的条件和维度错误；不是人工黄金标注，也不保证消除所有语义歧义。"}
    target.mkdir(parents=True,exist_ok=True);(target/"views").mkdir(exist_ok=True)
    if (target/"inputs.json").exists():assert json.loads((target/"inputs.json").read_text())==inputs,"Reviewed snapshot changed"
    def put(path,value):path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+"\n")
    put(target/"inputs.json",inputs)
    for ident,value in raw.items():put(target/"views"/(ident+".json"),value)
    rows=[dict(id=ident,status="complete") for ident in sorted(raw)]
    put(target/"view-summary.json",dict(status="complete",rows=rows))
    put(target/"summary.json",dict(status="views_reviewed",rows=rows,not_a_comparison_run=True))
    put(target/"semantic-review.json",dict(decision="approved_for_feasibility_only",corrections=changes,
        inputs_sha256=hashlib.sha256((target/"inputs.json").read_bytes()).hexdigest(),
        view_sha256={ident:hashlib.sha256((target/"views"/(ident+".json")).read_bytes()).hexdigest() for ident in raw}))
    print("Frozen reviewed views",TARGET,"corrections",len(changes))

if __name__=="__main__":main()
