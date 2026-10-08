#!/usr/bin/env python3
"""P2 analysis with article-level bootstrap, topic strata, noise and order sensitivity."""
import argparse
import hashlib
import json
from pathlib import Path
import numpy as np

ROOT = Path("research/style-aggregation")


def mean(values):
    return float(np.mean(values)) if values else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    args = ap.parse_args()
    run = ROOT / "runs" / args.run
    summary = json.loads((run / "summary.json").read_text())
    assert summary["status"] == "complete", "Incomplete comparisons cannot be reported as complete"
    paths = sorted((run / "pairs").glob("*.json"))
    pairs = [json.loads(p.read_text()) for p in paths]
    candidate_mode = all(isinstance(p["metrics"], dict) for p in pairs)
    assert candidate_mode or all(isinstance(p["metrics"], list) for p in pairs), "Mixed metric versions"
    if candidate_mode:
        for p in pairs:
            assert p["metrics"]["metric_version"] == "candidate_maximum_overlap_v2"
            p["candidate_diagnostics"] = p["metrics"]["candidate_conflicts_and_uncertainty"]
            p["metrics"] = p["metrics"]["overlap_only"]
    primary = [p for p in pairs if p["kind"] == "main"]
    n_repeats = sum(p["kind"] == "repeat_noise" for p in pairs)
    assert len(primary) == 190 and 4 <= n_repeats <= 8 and len(pairs) == 200 + n_repeats
    by_pair = {tuple(sorted((p["left"], p["right"]))): p for p in primary}
    assert len(by_pair) == 190
    ds_path = ROOT / "datasets/authors-v1/manifest.json"
    articles = {a["article_id"]: a for a in json.loads(ds_path.read_text())["articles"] if a["split"] == "discovery"}
    ids = sorted(articles)
    assert len(ids) == 20
    author_ids = sorted({a["author_id"] for a in articles.values()})
    groups = [[ids.index(i) for i in ids if articles[i]["author_id"] == a] for a in author_ids]
    assert list(map(len, groups)) == [10, 10]
    for p in primary:
        assert p["within_author"] == (articles[p["left"]]["author_id"] == articles[p["right"]]["author_id"])
    assert sum(p["within_author"] for p in primary) == 90
    rng = np.random.default_rng(17)
    weights = np.zeros((2000, 20), dtype=int)
    for group in groups:
        weights[:, group] = rng.multinomial(10, [0.1] * 10, size=2000)
    output = []
    for dim in [m["dimension"] for m in primary[0]["metrics"]]:
        metric = lambda p: next(m for m in p["metrics"] if m["dimension"] == dim)
        valid = [p for p in primary if metric(p)["coverage"] == "both"]
        record = {"dimension": dim, "both_covered_pairs": len(valid), "all_pairs": 190,
                  "one_sided_pairs": sum(metric(p)["coverage"] == "one_sided" for p in primary),
                  "neither_pairs": sum(metric(p)["coverage"] == "neither" for p in primary)}
        boot = {}
        for label, within in [("within", True), ("between", False)]:
            subset = [p for p in valid if p["within_author"] == within]
            values = [metric(p)["overlap"] for p in subset]
            record[label] = {"mean": mean(values), "n_pairs": len(values)}
            if subset:
                w = np.stack([weights[:, ids.index(p["left"])] * weights[:, ids.index(p["right"])] for p in subset], axis=1)
                numerator = w @ np.array(values)
                denominator = w.sum(axis=1)
                boot[label] = np.divide(numerator, denominator, out=np.full(2000, np.nan), where=denominator > 0)
            else:
                boot[label] = np.full(2000, np.nan)
        difference = boot["within"] - boot["between"]
        finite = difference[np.isfinite(difference)]
        record["difference_article_bootstrap_interval"] = [float(x) for x in np.quantile(finite, [0.025, 0.975])] if len(finite) else None
        record["valid_bootstrap_replicates"] = len(finite)
        record["topic_strata"] = [
            {"within_author": within, "same_topic": same,
             "n_pairs": len(values), "mean": mean(values)}
            for within in [True, False] for same in [True, False]
            for values in [[metric(p)["overlap"] for p in valid if p["within_author"] == within
                            and (articles[p["left"]]["topic"] == articles[p["right"]]["topic"]) == same]]]
        repeated = [metric(p)["overlap"] for p in pairs if p["kind"] == "repeat_noise" and metric(p)["coverage"] == "both"]
        record["same_article_repeat"] = {"n_pairs": len(repeated), "mean_overlap": mean(repeated), "values": repeated, "missing_pairs":8-n_repeats}
        deltas = []
        for p in pairs:
            if p["kind"] != "order_check":
                continue
            original = by_pair[tuple(sorted((p["left"], p["right"])))]
            x, y = metric(p)["overlap"], metric(original)["overlap"]
            if x is not None and y is not None:
                deltas.append(abs(x-y))
        record["left_right_swap"] = {"n_pairs": len(deltas), "mean_absolute_difference": mean(deltas), "values": deltas}
        if candidate_mode:
            diagnostics = [next(m for m in p["candidate_diagnostics"] if m["dimension"] == dim) for p in primary]
            record["opposite_candidate_edges"] = sum(d["opposite_edges"] for d in diagnostics)
            record["uncertain_candidate_edges"] = sum(d["uncertain_edges"] for d in diagnostics)
            total = sum(metric(p)["left_count"] * metric(p)["right_count"] for p in primary)
            record["candidate_count"] = total
            record["undecided_candidate_share"] = record["uncertain_candidate_edges"] / total if total else None
        else:
            record["conflicts"] = sum(metric(p)["conflicts"] for p in primary)
            total = sum(metric(p)["left_count"] + metric(p)["right_count"] for p in primary)
            unknown = sum(metric(p)["uncertain_left"] + metric(p)["uncertain_right"] for p in primary)
            record["undecided_feature_share"] = unknown / total if total else None
        output.append(record)
    out = ROOT / "reports" / args.run
    out.mkdir(parents=True, exist_ok=True)
    result = {"schema": 1, "run": args.run, "bootstrap": {"seed": 17, "replicates": 2000, "unit": "article, within each of two fixed authors", "self_pairs": "excluded"},
              "dimensions": output, "limitations": ["探索性文章重采样区间，不能外推作者总体", "190对不是190个独立样本", "单边缺失不计入两边都有描述的均值，覆盖另报", f"同文重复只有4篇文章，有效{n_repeats}/8对，缺失不填零；各来源重复次数可能不等", "题材分层只能诊断混淆，不能证明已经排除"],
              "input_sha256": {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths + [ds_path]},
              "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    result["metric_version"] = "candidate_maximum_overlap_v2" if candidate_mode else "one_to_one_jaccard_per_axis_v1"
    if candidate_mode:
        result["limitations"].append("新版冲突/未知按候选边计数，不能与旧一对一关系或特征未知率混用")
    (out / "analysis.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    lines = ["# P2逐维比较结果", "", "以下为当前两位作者、20篇文章的探索性结果。原始点、覆盖、重复波动与位置敏感性分开报告，不把表征区分当作生成保真。", "", "|维度|作者内均值|作者间均值|可比较配对|同文重复均值|左右交换平均变化|", "|---|---:|---:|---:|---:|---:|"]
    fmt = lambda v: "NA" if v is None else f"{v:.3f}"
    for r in output:
        lines.append(f"|{r['dimension']}|{fmt(r['within']['mean'])}|{fmt(r['between']['mean'])}|{r['both_covered_pairs']}/190|{fmt(r['same_article_repeat']['mean_overlap'])}|{fmt(r['left_right_swap']['mean_absolute_difference'])}|")
    lines += ["", "维度差值的文章重采样区间、题材分层和全部噪声点见analysis.json。没有把缺失填零；没有合成一个作者相似总分。", "", "阶段控制通过只支持在当前控制范围内使用比较器；仍需复核具体证据及P3–P5结果。"]
    (out / "report.md").write_text("\n".join(lines) + "\n")
    print(out)


if __name__ == "__main__":
    main()
