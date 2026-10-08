#!/usr/bin/env python3
"""Synthetic arithmetic checks in a temporary directory, never in real research runs."""
import importlib.util
import itertools
import json
import sys
import tempfile
from pathlib import Path
sys.dont_write_bytecode = True


def load(name):
    spec=importlib.util.spec_from_file_location(name,Path(__file__).with_name(name+".py"))
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def put(path,value):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(value))


def metric(dim,value):
    return dict(dimension=dim,coverage="both",overlap=value,conflicts=0,left_count=1,right_count=1,uncertain_left=0,uncertain_right=0)


def main():
    analysis=load("analyze_style_p2");reports=load("report_style_followthrough")
    with tempfile.TemporaryDirectory() as folder:
        root=Path(folder);analysis.ROOT=root;reports.ROOT=root
        articles=[dict(article_id=f"{a}-{i}",author_id=a,topic=str(i%3),split="discovery") for a in ["a","b"] for i in range(10)]
        put(root/"datasets/authors-v1/manifest.json",dict(articles=articles))
        run=root/"runs/pairs";put(run/"summary.json",dict(status="complete"))
        pairs=[]
        for i,(a,b) in enumerate(itertools.combinations(articles,2)):
            within=a["author_id"]==b["author_id"]
            pairs.append(dict(id=f"p{i}",kind="main",left=a["article_id"],right=b["article_id"],within_author=within,metrics=[metric("voice",int(within))]))
        for i in range(10):
            p={**pairs[i],"id":f"swap{i}","kind":"order_check","left":pairs[i]["right"],"right":pairs[i]["left"]};pairs.append(p)
        for i in range(8):pairs.append({**pairs[i],"id":f"repeat{i}","kind":"repeat_noise"})
        for p in pairs:put(run/"pairs"/(p["id"]+".json"),p)
        old=sys.argv;sys.argv=["analysis","pairs"]
        try:analysis.main()
        finally:sys.argv=old
        result=json.loads((root/"reports/pairs/analysis.json").read_text())["dimensions"][0]
        assert result["difference_article_bootstrap_interval"]==[1.0,1.0]
        assert result["within"]["n_pairs"]==90 and result["between"]["n_pairs"]==100
        assert result["left_right_swap"]["mean_absolute_difference"]==0
        for p in pairs:
            converted={**p,"metrics":{"metric_version":"candidate_maximum_overlap_v2","overlap_only":p["metrics"],
                "candidate_conflicts_and_uncertainty":[dict(dimension="voice",opposite_edges=2,uncertain_edges=1)]}}
            put(run/"pairs"/(p["id"]+".json"),converted)
        sys.argv=["analysis","pairs"]
        try:analysis.main()
        finally:sys.argv=old
        result=json.loads((root/"reports/pairs/analysis.json").read_text())["dimensions"][0]
        assert result["difference_article_bootstrap_interval"]==[1.0,1.0]
        assert result["opposite_candidate_edges"]==380 and result["undecided_candidate_share"]==1
        assert "conflicts" not in result and "undecided_feature_share" not in result
        plans=[];comparisons=[]
        for author in ["a","b"]:
            for k in [1,2,3,5,7,10]:
                ident=f"{author}-{k}";plans.append(dict(id=ident,author=author))
                put(root/"runs/profiles/profiles"/(ident+".json"),dict(profile=None if k==1 else dict(rules=[])))
            for low,high in zip([1,2,3,5,7],[2,3,5,7,10]):
                comparisons.append(dict(pair=dict(left=f"{author}-{low}",right=f"{author}-{high}",references=[dict(kind="adjacent",author=author,seed=s,from_k=low,to_k=high) for s in [17,29,43,71,101]]),metrics=[metric(d,1) for d in reports.NAMES]))
            for mode in ["repeat","reverse","shuffle"]:
                comparisons.append(dict(pair=dict(left=f"{author}-10",right=f"{author}-10",references=[dict(kind="aggregation_noise",author=author,mode=mode,k=10)]),metrics=[metric(d,1) for d in reports.NAMES]))
        comparisons.append(dict(pair=dict(left="a-10",right="b-10",references=[dict(kind="between_authors",seed=17,k=10)]),metrics=[metric(d,0) for d in reports.NAMES]))
        put(root/"runs/profiles/summary.json",dict(status="complete"));put(root/"runs/profiles/plans.json",plans)
        put(root/"runs/convergence/summary.json",dict(status="complete"))
        for i,p in enumerate(comparisons):put(root/"runs/convergence/pairs"/(str(i)+".json"),p)
        out=reports.convergence("profiles","convergence");result=json.loads((out/"analysis.json").read_text())
        assert all(r["last_two_intervals_candidate"] for r in result["candidates"])
        for p in comparisons:
            if p["pair"]["references"][0]["kind"]=="between_authors":
                p["metrics"]=[metric(d,1) for d in reports.NAMES]
        for i,p in enumerate(comparisons):put(root/"runs/convergence/pairs"/(str(i)+".json"),p)
        out=reports.convergence("profiles","convergence");result=json.loads((out/"analysis.json").read_text())
        assert not any(r["last_two_intervals_candidate"] for r in result["candidates"]),"Collapsed author distinction must not pass convergence"
    print("Synthetic checks passed: article bootstrap, swap invariance, convergence and author-collapse rejection; temporary data removed")


if __name__=="__main__":main()
