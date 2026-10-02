#!/usr/bin/env python3
"""Deterministic P4/P5 reports from complete stored experiments; no synthetic scores."""
import argparse
import hashlib
import json
import statistics
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path("research/style-aggregation")
NAMES = {"language":"语域用词", "voice":"人称语气", "rhythm":"句段节奏", "information":"信息展开",
         "narration":"叙述议论", "constraints":"条件边界", "title":"标题", "opening":"开头", "ending":"结尾"}
SCORES = {"style_fidelity":"原文风格保真", "source_independence":"原文独立性", "factual_compliance":"事实合规", "naturalness":"自然度", "task_completion":"主题完成度"}
plt.rcParams.update({"font.family":"Noto Sans SC", "axes.unicode_minus":False})


def complete(name):
    root = ROOT / "runs" / name
    assert json.loads((root / "summary.json").read_text())["status"] == "complete", name
    return root


def med(xs):
    return statistics.median(xs) if xs else None


def avg(xs):
    return statistics.mean(xs) if xs else None


def convergence(profile_run, compare_run):
    profile_root, compare_root = complete(profile_run), complete(compare_run)
    plans = json.loads((profile_root / "plans.json").read_text())
    artifacts = {p["id"]: json.loads((profile_root / "profiles" / (p["id"] + ".json")).read_text()) for p in plans}
    pairs = [json.loads(p.read_text()) for p in sorted((compare_root / "pairs").glob("*.json"))]
    records = []
    for p in pairs:
        for ref in p["pair"]["references"]:
            for metric in p["metrics"]:
                records.append({**ref, **metric, "left":p["pair"]["left"], "right":p["pair"]["right"]})
    authors = sorted({p["author"] for p in plans})
    rows = []
    fig, axes = plt.subplots(3, 3, figsize=(13, 11))
    for ax, (dim, label) in zip(axes.flat, NAMES.items()):
        for author in authors:
            eligible = [r for r in records if r["dimension"] == dim and r["coverage"] == "both"]
            noise = [1-r["overlap"] for r in eligible if r["kind"] == "aggregation_noise" and r["author"] == author]
            threshold = min(0.15, med(noise)+0.05) if noise else None
            between = [r["overlap"] for r in eligible if r["kind"] == "between_authors" and r["k"] == 10]
            # Same full-set pair can carry five seed references; never call these five independent measurements.
            full_gap = (1-med(noise)-med(between)) if noise and between else None
            changes = []
            for low, high in zip([1,2,3,5,7], [2,3,5,7,10]):
                selected = [r for r in eligible if r["kind"] == "adjacent" and r["author"] == author and r["from_k"] == low and r["to_k"] == high]
                values = [1-r["overlap"] for r in selected]
                conflict_increases = []
                for r in selected:
                    def conflicts(ident):
                        profile = artifacts[ident]["profile"]
                        return sum(x["classification"] == "conflict" and x["candidate"]["dimension"] == dim for x in profile["rules"]) if profile else None
                    left, right = conflicts(r["left"]), conflicts(r["right"])
                    if left is not None and right is not None:
                        conflict_increases.append(right > left)
                row = {"author":author, "dimension":dim, "from_k":low, "to_k":high,
                       "median_change":med(values), "values":values, "n_seed_paths":len(values),
                       "noise_median":med(noise), "threshold":threshold,
                       "conflict_increased":any(conflict_increases) if conflict_increases else None,
                       "full_within_minus_between":full_gap}
                row["interval_candidate"] = (len(values) == 5 and threshold is not None and med(values) <= threshold
                    and row["conflict_increased"] is False and full_gap is not None and full_gap > 0)
                rows.append(row)
                changes.append(med(values))
            ax.plot([2,3,5,7,10], [float("nan") if v is None else v for v in changes], marker="o", label="王小波" if author=="wang_xiaobo" else "汪曾祺")
        ax.set_title(label); ax.set_ylim(0,1); ax.set_xticks([2,3,5,7,10]); ax.set_xlabel("增长后的样本数")
    axes.flat[0].legend(fontsize=8)
    fig.suptitle("相邻样本量的规则变动率中位数（5条固定抽样路径）\n低值表示表示变化较小，不代表生成效果好")
    fig.tight_layout(rect=(0,0,1,.94))
    out = ROOT / "reports" / compare_run; out.mkdir(parents=True, exist_ok=True)
    fig.savefig(out / "convergence.png", dpi=170); plt.close(fig)
    candidates = []
    for a in authors:
        for d in NAMES:
            tail = [r for r in rows if r["author"]==a and r["dimension"]==d and r["to_k"] in [7,10]]
            candidates.append({"author":a,"dimension":d,"last_two_intervals_candidate":len(tail)==2 and all(r["interval_candidate"] for r in tail)})
    (out / "analysis.json").write_text(json.dumps({"rows":rows,"candidates":candidates,
        "scope":"逐维探索性候选，非整体作者真值或产品采用门槛",
        "limitations":["k1保留原单篇粒度，聚合最多12条，粒度变化影响重合", "重复集合共享结果，五条路径不等于五个独立研究", "跨作者未塌缩操作检查为k10同作者重复重合中位数高于跨作者重合"]}, ensure_ascii=False, indent=2)+"\n")
    text = ["# P4收敛观察", "", "![逐维变动率](convergence.png)", "", "|作者|维度|最后两个增长区间满足候选条件|", "|---|---|---|"]
    for c in candidates:
        text.append(f"|{'王小波' if c['author']=='wang_xiaobo' else '汪曾祺'}|{NAMES[c['dimension']]}|{'是，待结合其他证据解读' if c['last_two_intervals_candidate'] else '未观察到或证据不足'}|")
    text += ["", "条件包括变动率中位数≤0.15且≤重复噪声+0.05，冲突不增加，k10同作者重复重合高于跨作者；缺失不填零。详细分母、原始点及噪声见analysis.json。", "", "不能仅凭规则数量受上限限制或稳定标签增加宣布收敛；生成结果另见P5。"]
    (out / "report.md").write_text("\n".join(text)+"\n")
    return out


def evaluation(reference_run, confirmation_run):
    rows = []
    for name in [reference_run, confirmation_run]:
        root = complete(name)
        assert json.loads((root / "controls.json").read_text())["passed"]
        rows += [json.loads(p.read_text()) for p in sorted((root / "judgments").glob("*.json"))]
    assert len(rows)==60 and len({r["id"] for r in rows})==60
    out = ROOT / "reports/P5-results"; out.mkdir(parents=True, exist_ok=True)
    comparisons = []
    for author in sorted({r["author"] for r in rows}):
        for topic in sorted({r["topic"] for r in rows}):
            group = [r for r in rows if r["author"]==author and r["topic"]==topic]
            by_method = {m:[r for r in group if r["method"]==m] for m in ["single","aggregate","neutral"]}
            assert [len(by_method[m]) for m in ["single","aggregate","neutral"]]==[6,2,2]
            values = {m:{d:avg([r["transfer"]["scores"][d] for r in rs]) for d in SCORES} for m,rs in by_method.items()}
            for m, rs in by_method.items():
                values[m].update(target_fidelity=avg([r["target_fidelity"] for r in rs]),
                                 author_match_rate=avg([int(r["author_match"]["choice"]==r["target_label"]) for r in rs]),
                                 uncertain_rate=avg([int(r["author_match"]["choice"] in ["uncertain","both","neither"]) for r in rs]),
                                 length_pass_rate=avg([int(r["length"]["within_range"]) for r in rs]))
            comparisons.append({"author":author,"topic":topic,"phase":group[0]["phase"],"means":values,
                "aggregate_minus_single":{d:values["aggregate"][d]-values["single"][d] for d in list(SCORES)+["target_fidelity","author_match_rate","length_pass_rate"]}})
    (out / "analysis.json").write_text(json.dumps({"rows":rows,"author_topic_comparisons":comparisons,
        "limitations":["模型评分不是人工验收", "仅两位作者三主题，不能外推普遍效果", "三份单篇是同作者的基线分布，不是三个作者", "各指标独立，无采用总分", "reference开发与confirmation确认分别报告"]}, ensure_ascii=False, indent=2)+"\n")
    fig, axes = plt.subplots(2,3,figsize=(13,8))
    dims = list(SCORES)+["target_fidelity"]
    for ax,d in zip(axes.flat,dims):
        for x,phase in enumerate(["reference","confirmation"]):
            vals=[c["aggregate_minus_single"][d] for c in comparisons if c["phase"]==phase]
            ax.scatter([x]*len(vals),vals,s=32,alpha=.6)
            if vals: ax.plot([x-.15,x+.15],[avg(vals)]*2,color="black")
        ax.axhline(0,color="#999",linestyle="--");ax.set_xticks([0,1],["开发主题","确认主题"]);ax.set_title(SCORES.get(d,"独立参考作者保真"));ax.set_ylim(-4,4)
    fig.suptitle("聚合减去三份单篇基线均值：每点为作者×主题\n正值表示当前指标更高，不合成总体胜负")
    fig.tight_layout(rect=(0,0,1,.93));fig.savefig(out/"generation-differences.png",dpi=170);plt.close(fig)
    text=["# P5生成对照", "", "![分项差值](generation-differences.png)", "", "48篇主对照和12篇无风格控制完整评分；开发与确认分开。每位作者每题先汇总3份单篇×2次生成，再与聚合×2次生成比较。", "", "|作者|题目|阶段|风格差值|事实差值|独立作者保真差值|", "|---|---|---|---:|---:|---:|"]
    for c in comparisons:
        v=c["aggregate_minus_single"];text.append(f"|{c['author']}|{c['topic']}|{c['phase']}|{v['style_fidelity']:.2f}|{v['factual_compliance']:.2f}|{v['target_fidelity']:.2f}|")
    text += ["", "完整五维、字数、无风格控制、无法判断比例及所有原始评分见analysis.json。未收集人工反馈时，本报告只能称模型评估结果，不能称人评通过。"]
    (out/"report.md").write_text("\n".join(text)+"\n")
    return out


def main():
    ap=argparse.ArgumentParser();ap.add_argument("kind",choices=["convergence","evaluation"]);ap.add_argument("first");ap.add_argument("second");args=ap.parse_args()
    out=convergence(args.first,args.second) if args.kind=="convergence" else evaluation(args.first,args.second)
    (out/"provenance.json").write_text(json.dumps({"runs":[args.first,args.second],"script_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},indent=2)+"\n")
    print(out)


if __name__=="__main__":main()
