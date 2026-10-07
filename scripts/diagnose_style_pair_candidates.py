#!/usr/bin/env python3
"""Offline feasibility diagnostic only: never converts invalid responses into research scores."""
import hashlib
import json
from pathlib import Path
from collections import Counter, defaultdict

ROOT=Path("research/style-aggregation")


def maximum_matching(edges):
    adjacency=defaultdict(set)
    for left,right in edges:adjacency[left].add(right)
    assigned={}
    def augment(left,seen):
        for right in sorted(adjacency[left]):
            if right in seen:continue
            seen.add(right)
            if right not in assigned or augment(assigned[right],seen):
                assigned[right]=left;return True
        return False
    for left in sorted(adjacency):augment(left,set())
    return sorted((left,right) for right,left in assigned.items())


def main():
    # This example needs reassignment; a greedy first-edge choice would retain only one edge.
    assert len(maximum_matching([("a1","b1"),("a1","b2"),("a2","b1")]))==2
    assert len(maximum_matching([("a1","b1"),("a1","b2")]))==1
    assert maximum_matching([])==[]
    root=ROOT/"runs/p2-pairs-02"
    rows=[]
    for path in sorted((root/"requests").glob("pair-*-0.json")):
        cp=json.loads(path.read_text());receipt=cp["receipt"]
        assert hashlib.sha256(json.dumps(receipt,ensure_ascii=False,separators=(",",":")).encode()).hexdigest()==cp["receipt_sha256"]
        inputs=json.loads(cp["request"]["body"]["messages"][-1]["content"])
        a={f["id"]:f for f in inputs["A"]["features"]};b={f["id"]:f for f in inputs["B"]["features"]}
        try: response=json.loads(receipt["content"])
        except ValueError:
            rows.append(dict(request=path.stem,status="unparseable",request_sha256=hashlib.sha256(path.read_bytes()).hexdigest()));continue
        proposed=response.get("matches",[]);edges=defaultdict(set);rejected=[]
        for edge in proposed:
            left,right=edge.get("left"),edge.get("right")
            if left not in a or right not in b:
                rejected.append(dict(edge=edge,reason="unknown_id"));continue
            if a[left]["dimension"]!=b[right]["dimension"]:
                rejected.append(dict(edge=edge,reason="cross_dimension"));continue
            if not isinstance(edge.get("reason"),str) or not edge["reason"].strip():
                rejected.append(dict(edge=edge,reason="missing_reason"));continue
            edges[a[left]["dimension"]].add((left,right))
        potential={dim:maximum_matching(pairs) for dim,pairs in edges.items()}
        for selected in potential.values():
            assert len({x for x,y in selected})==len(selected)==len({y for x,y in selected})
        rows.append(dict(request=path.stem,status="diagnostic_only",proposed_matches=len(proposed),
            repeated_left=sum(v-1 for v in Counter(e.get("left") for e in proposed).values() if v>1),
            repeated_right=sum(v-1 for v in Counter(e.get("right") for e in proposed).values() if v>1),
            rejected=rejected,same_dimension_candidate_edges=sum(map(len,edges.values())),
            feasible_equivalence_subset=potential,feasible_count=sum(map(len,potential.values())),
            conflicts_not_adjudicated=response.get("conflicts",[]),
            request_sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    out=ROOT/"reports/P2-comparison-diagnosis";out.mkdir(parents=True,exist_ok=True)
    valid=[r for r in rows if r["status"]=="diagnostic_only"]
    summary=dict(response_count=len(rows),parseable=len(valid),
        responses_with_cross_dimension_matches=sum(any(e["reason"]=="cross_dimension" for e in r["rejected"]) for r in valid),
        responses_with_repeated_match_endpoints=sum(r["repeated_left"]>0 or r["repeated_right"]>0 for r in valid),
        status="offline_prototype_not_accepted_research_results",
        limitations=["最大匹配只证明程序能落实组合约束，不证明候选语义正确", "未处理等价与冲突候选之间的仲裁", "没有生成相似度分数，没有把非法响应补成正式成功结果", "没有新的模型请求"],
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    (out/"candidate-diagnostic.json").write_text(json.dumps(dict(summary=summary,rows=rows),ensure_ascii=False,indent=2)+"\n")
    print(json.dumps(summary,ensure_ascii=False))


if __name__=="__main__":main()
