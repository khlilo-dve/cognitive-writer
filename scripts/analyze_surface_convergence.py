#!/usr/bin/env python3
"""Frozen-subset descriptive convergence of raw, observable text measures."""
import csv
import hashlib
import json
import statistics
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"
METRICS = {
    "sentence_median": "句长中位数（字）",
    "paragraph_median": "段长中位数（字）",
    "first_person_per_1000": "我／千字",
    "question_per_1000": "问号／千字",
    "exclamation_per_1000": "感叹号／千字",
}


def main():
    manifest = ROOT / "datasets/authors-v1/manifest.json"
    sampling_path = ROOT / "datasets/sampling-v1.json"
    sampling = json.loads(sampling_path.read_text())
    manifest_sha = hashlib.sha256(manifest.read_bytes()).hexdigest()
    assert sampling["manifest_sha256"] == manifest_sha
    source_dir = ROOT / "reports/source-description-v1"
    provenance = json.loads((source_dir / "provenance.json").read_text())
    assert provenance["manifest_sha256"] == manifest_sha and provenance["articles"] == 20
    with (source_dir / "articles.csv").open(encoding="utf-8-sig") as f:
        originals = {r["article_id"]:r for r in csv.DictReader(f)}
    assert len(originals) == 20
    ids = {a["article_id"] for a in json.loads(manifest.read_text())["articles"] if a["split"] == "discovery"}
    assert set(originals) == ids
    full = {}
    rows = []
    for author, plan in sampling["authors"].items():
        order = plan["orders"]
        assert len(order) == 5
        ten = set(order[0]["subsets"]["10"])
        assert len(ten) == 10 and all(originals[id]["author_id"] == author for id in ten)
        for entry in order:
            assert set(entry["subsets"]["10"]) == ten
        full[author] = {m:statistics.mean(float(originals[id][m]) for id in ten) for m in METRICS}
        for entry in order:
            for k in (1,2,3,5,7,10):
                subset = entry["subsets"][str(k)]
                assert len(subset) == len(set(subset)) == k and set(subset) <= ten
                topics = {originals[id]["topic"] for id in subset}
                for metric in METRICS:
                    estimate = statistics.mean(float(originals[id][metric]) for id in subset)
                    rows.append(dict(author=author,seed=entry["seed"],k=k,metric=metric,
                        subset="|".join(subset),topics=len(topics),estimate=round(estimate,4),
                        full10=round(full[author][metric],4),absolute_gap=round(abs(estimate-full[author][metric]),4)))
    assert len(rows) == 2*5*6*len(METRICS)
    out = ROOT / "reports/surface-convergence-v1"
    out.mkdir(parents=True,exist_ok=True)
    with (out / "subsets.csv").open("w",newline="") as f:
        writer=csv.DictWriter(f,fieldnames=list(rows[0]));writer.writeheader();writer.writerows(rows)
    grouped=defaultdict(list)
    for row in rows:grouped[(row["author"],row["metric"],row["k"])].append(row["absolute_gap"])
    summary=[dict(author=a,metric=m,k=k,paths=5,median_absolute_gap=statistics.median(v),
                  min_gap=min(v),max_gap=max(v)) for (a,m,k),v in sorted(grouped.items())]
    result=dict(suite="surface_convergence_v1",dataset_manifest_sha256=manifest_sha,
        sampling_sha256=hashlib.sha256(sampling_path.read_bytes()).hexdigest(),
        source_description_sha256=hashlib.sha256((source_dir / "articles.csv").read_bytes()).hexdigest(),
        api_calls=0,full10=full,rows=summary,scope="within the frozen ten articles per author",
        caveats=["k10 gap equals zero by definition", "Five orders share articles and are not independent author samples",
            "These surface measures do not validate semantic Style rules or generated fidelity"])
    (out / "analysis.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    plt.rcParams.update({"font.family":"Noto Sans SC","axes.unicode_minus":False})
    selected=["sentence_median","paragraph_median","first_person_per_1000","question_per_1000"]
    fig,axes=plt.subplots(2,2,figsize=(11,7),layout="constrained")
    for ax,metric in zip(axes.flat,selected):
        for author,label,color in [("wang_xiaobo","王小波","#2f6c91"),("wang_zengqi","汪曾祺","#bc8553")]:
            line=[next(x for x in summary if x["author"]==author and x["metric"]==metric and x["k"]==k) for k in (1,2,3,5,7,10)]
            ax.plot([x["k"] for x in line],[x["median_absolute_gap"] for x in line],marker="o",label=label,color=color)
        ax.set_title(METRICS[metric]);ax.set_xticks((1,2,3,5,7,10));ax.set_xlabel("文章数")
        ax.set_ylabel("相对这10篇均值的差距")
        ax.spines[["top","right"]].set_visible(False)
    axes.flat[0].legend()
    fig.suptitle("可观察文字量随样本数变化\n10篇差距为0是定义，不代表作者风格已稳定")
    chart=ROOT / "charts/surface-convergence-v1";chart.mkdir(parents=True,exist_ok=True)
    fig.savefig(chart / "surface-convergence.png",dpi=170)
    plt.close(fig)
    lines=["# P4：冻结10篇内的可观察文字量", "", "两位作者各10篇，按原计划5个顺序与1/2/3/5/7/10篇子集。程序统计句长、段长、人称与标点，不调用模型、不读取留出正文。", "",
        "图中的距离是子集均值和这10篇完整均值的差。它回答‘这批原文的这些量何时接近全量均值’；不能证明语义风格规则收敛，也不能预测新题材写作。", "", "|作者|指标|1篇差距中位数|5篇差距中位数|7篇差距中位数|", "|---|---|---:|---:|---:|"]
    for author in ("wang_xiaobo","wang_zengqi"):
        for metric in METRICS:
            lookup={k:next(x for x in summary if x["author"]==author and x["metric"]==metric and x["k"]==k)["median_absolute_gap"] for k in (1,5,7)}
            lines.append(f"|{'王小波' if author=='wang_xiaobo' else '汪曾祺'}|{METRICS[metric]}|{lookup[1]:.2f}|{lookup[5]:.2f}|{lookup[7]:.2f}|")
    lines += ["", "每个点来自5个共享文章的固定顺序，不是5位独立作者；10篇差距按定义为0。题材覆盖、引用和文章长短都可影响指标。原始60个子集×5项指标在subsets.csv，图在charts/surface-convergence-v1/。"]
    (out / "report.md").write_text("\n".join(lines)+"\n")
    print(json.dumps({"rows":len(rows),"full10":full,"api_calls":0},ensure_ascii=False))


if __name__=="__main__":main()
