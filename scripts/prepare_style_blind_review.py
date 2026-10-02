#!/usr/bin/env python3
"""Build eight fixed blind comparisons from frozen generation, keeping mapping private."""
import hashlib
import json
from pathlib import Path

ROOT=Path("research/style-aggregation")


def main():
    run=ROOT/"runs/p5-generation-01"
    summary=json.loads((run/"summary.json").read_text())
    if summary["status"]!="complete" or len(summary["rows"])!=60:
        raise ValueError("All 60 generated articles must be frozen before creating blind material")
    articles=[json.loads(p.read_text()) for p in sorted((run/"articles").glob("*.json"))]
    assert len(articles)==60 and len({a["job"]["id"] for a in articles})==60
    dataset=ROOT/"datasets/authors-v1/manifest.json"
    ds=json.loads(dataset.read_text())
    sampling=json.loads((ROOT/"datasets/sampling-v1.json").read_text())
    assert sampling["manifest_sha256"]==hashlib.sha256(dataset.read_bytes()).hexdigest()
    def find(author,topic,rep,method,style=None):
        found=[a for a in articles if a["job"]["author"]==author and a["job"]["topic_id"]==topic
               and a["job"]["replicate"]==rep and a["job"]["method"]==method
               and (style is None or a["job"]["style_id"]==style)]
        assert len(found)==1
        return found[0]
    public=[];private=[]
    for ai,author in enumerate(sorted(sampling["authors"])):
        reference=[a for a in ds["articles"] if a["author_id"]==author and a["split"]=="confirmation"]
        assert len(reference)==1
        raw=(dataset.parent/reference[0]["path"]).read_bytes()
        assert hashlib.sha256(raw).hexdigest()==reference[0]["sha256"]
        baselines=[value for key,value in sorted(sampling["authors"][author]["generation_baselines"].items())]
        for ti,topic in enumerate(["study","repair"]):
            for rep in [1,2]:
                single=find(author,topic,rep,"single",baselines[(ti*2+rep-1+ai)%3])
                aggregate=find(author,topic,rep,"aggregate")
                index=len(public);left,right=(aggregate,single) if index%2==0 else (single,aggregate)
                ident=f"Q{index+1:02}"
                public.append(dict(id=ident,reference=raw.decode("utf-8"),task=single["job"]["task"],A=left["body"],B=right["body"],
                    question="哪篇文章的表达方式更接近参考材料？",choices=["A","B","接近","两篇都不像","无法判断"],
                    confidence_choices=["较有把握","一般","不太确定"],optional_reason="可以指出具体句子或表达方式；不必评价观点是否正确。"))
                private.append(dict(id=ident,author=author,topic=topic,replicate=rep,A_id=left["job"]["id"],B_id=right["job"]["id"],
                    A_method=left["job"]["method"],B_method=right["job"]["method"],baseline_style=single["job"]["style_id"]))
    orders=[[0,3,4,7],[1,2,5,6],[7,4,3,0],[6,5,2,1]]
    versions={f"V{i+1}":[public[n]["id"] for n in order] for i,order in enumerate(orders)}
    for order in orders:
        assert sum(private[n]["A_method"]=="aggregate" for n in order)==2
    out=ROOT/"blind-review";out.mkdir(parents=True,exist_ok=True)
    def freeze(name,value):
        p=out/name
        if p.exists() and json.loads(p.read_text())!=value:
            raise ValueError(f"Frozen blind material changed: {name}")
        p.write_text(json.dumps(value,ensure_ascii=False,indent=2)+"\n")
    freeze("public-materials.json",dict(instructions="关注表达方式，不是观点赞同或文章好不好看。每人只填写一个版本，共4组。自填6–8位随机参与码，不填姓名、手机号或联系方式。平台可能记录登录与提交信息，不承诺完全匿名。",versions=versions,questions=public))
    freeze("private-mapping.json",dict(mapping=private,generation_run="p5-generation-01",generation_sha256=hashlib.sha256(json.dumps(articles,ensure_ascii=False,sort_keys=True).encode()).hexdigest(),
        sampling_policy="topic order study/repair, reps1/2, baseline rotation (topic_index*2+rep-1+author_index)%3, alternating left/right"))
    freeze("registration.json",dict(status="draft_not_distributed",target_participants=[8,12],questions_per_participant=4,versions=list(versions),
        assignment="按参与次序循环分配V1到V4",deadline=None,collection_days=7,
        deduplication="同一参与码仅保留最早完整提交的一份；同码跨版本也如此。未完成单列，不因选择方向或低置信排除。",
        before_distribution=["填写固定截止日期", "核验表单仅可提交且不泄露private-mapping或研究Base", "链接交用户分发，不自动发消息招募"]))
    print("Prepared 8 blind pairs and 4 balanced versions; mapping private, no invitations sent")


if __name__=="__main__":main()
