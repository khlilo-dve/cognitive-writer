#!/usr/bin/env python3
"""Offline wording diagnostic. No API, semantic judgments or holdout text."""
import csv
import hashlib
import json
import math
import re
from collections import Counter, defaultdict
from itertools import combinations
from pathlib import Path
from statistics import mean

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"


def grams(text):
    counts = Counter()
    for segment in re.findall(r"[^\W_]+", text.lower(), flags=re.UNICODE):
        for n in (2, 3, 4):
            counts.update(segment[i:i+n] for i in range(len(segment)-n+1))
    return counts


def main():
    source = ROOT / "runs/p2-views-02/inputs.json"
    items = json.loads(source.read_text())["items"]
    primary = [x for x in items if x["replicate"] == 1]
    repeats = [x for x in items if x["replicate"] != 1]
    assert len(primary) == 20 and len(repeats) == 8
    assert len({x["id"] for x in items}) == 28
    for item in items:
        original = ROOT / "runs" / item["source_run"] / "single" / (item["id"] + ".md")
        assert original.read_text() == item["spec"], item["id"]
    counts = {x["id"]: grams(x["spec"]) for x in items}
    df = Counter()
    for x in primary:
        df.update(counts[x["id"]].keys())
    idf = {g: 1 + math.log(21/(1+f)) for g, f in df.items()}
    vectors = {}
    for key, count in counts.items():
        vec = {g: (1+math.log(c))*idf[g] for g, c in count.items() if g in idf}
        norm = math.sqrt(sum(v*v for v in vec.values()))
        vectors[key] = {g: v/norm for g, v in vec.items()} if norm else {}
    manifest = json.loads((ROOT / "datasets/authors-v1/manifest.json").read_text())
    topics = {a["article_id"]: a["topic"] for a in manifest["articles"] if a["split"] == "discovery"}
    by_id = {x["id"]: x for x in primary}
    rows, shared = [], []

    def pair(a, b, group):
        av, bv = vectors[a["id"]], vectors[b["id"]]
        contribution = {g: v*bv[g] for g, v in av.items() if g in bv}
        rows.append(dict(left=a["id"], right=b["id"], group=group,
                         same_topic=topics[a["id"].split("-r")[0]] == topics[b["id"].split("-r")[0]],
                         cosine=sum(contribution.values())))
        shared.append(dict(left=a["id"], right=b["id"], top_shared_grams=sorted(contribution.items(), key=lambda p: (-p[1], p[0]))[:15]))

    for a, b in combinations(primary, 2):
        pair(a, b, "within_author" if a["author"] == b["author"] else "between_authors")
    for b in repeats:
        pair(by_id[b["id"].split("-r")[0]], b, "repeat")
    grouped = defaultdict(list)
    for row in rows:
        grouped[row["group"]].append(row["cosine"])
        if row["group"] != "repeat":
            grouped[row["group"] + ("/same_topic" if row["same_topic"] else "/different_topic")].append(row["cosine"])
    stats = {k: {"n": len(v), "mean": mean(v), "min": min(v), "max": max(v)} for k, v in sorted(grouped.items())}
    out = ROOT / "reports/lexical-diagnostic-v1"
    out.mkdir(parents=True, exist_ok=True)
    with (out / "pairs.csv").open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    result = dict(suite="lexical_diagnostic_v1", source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                  protocol_sha256=hashlib.sha256((ROOT / "method-revision-07.md").read_bytes()).hexdigest(),
                  api_calls=0, stats=stats, common_grams=sorted(df.items(), key=lambda p: (-p[1], p[0]))[:60])
    (out / "summary.json").write_text(json.dumps(result, ensure_ascii=False, indent=2)+"\n")
    (out / "shared-grams.json").write_text(json.dumps(shared, ensure_ascii=False, indent=2)+"\n")
    labels = {"within_author": "同作者不同文章", "between_authors": "不同作者", "repeat": "同文章重复提取"}
    lines = ["# 风格说明的文字相似度（免费诊断）", "", "只检查文字用法是否接近，不代表原作者风格是否学对。20份首次说明拟合，8份重复仅用于检查。未读取留出正文。", "", "|比较组|配对数|平均文字相似度|", "|---|---:|---:|"]
    for k, s in stats.items():
        lines.append(f"|{labels.get(k, k)}|{s['n']}|{s['mean']:.4f}|")
    lines += ["", "同一篇文章出现在多个配对中，配对不是独立样本；这里没有统计显著性或采用门槛。共同标题、事实约束、题材词汇和提取模型的惯用表达都能抬高相似度。shared-grams.json保存每对贡献最大的片段，不能把这些片段自动认作作者特征。", "", "后续聚合必须单独对照原文证据；本诊断不能恢复已失效的语义比较，也不能回答增加文章数量后的风格收敛。"]
    (out / "report.md").write_text("\n".join(lines)+"\n")
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    plt.rcParams.update({"font.family": "Noto Sans SC", "axes.unicode_minus": False})
    fig, ax = plt.subplots(figsize=(9, 4.8), layout="constrained")
    keys = list(labels)
    bars = ax.bar([labels[k] for k in keys], [stats[k]["mean"] for k in keys], color=["#376b91", "#8596a0", "#bd8953"])
    ax.bar_label(bars, fmt="%.4f", padding=5)
    ax.set_ylim(0, .16)
    ax.set_ylabel("平均字符片段余弦相似度")
    ax.set_title("说明书文字相似度：不是作者风格质量分", pad=18)
    ax.spines[["top", "right"]].set_visible(False)
    charts = ROOT / "charts/lexical-diagnostic-v1"
    charts.mkdir(parents=True, exist_ok=True)
    fig.savefig(charts / "wording.png", dpi=170)
    plt.close(fig)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
