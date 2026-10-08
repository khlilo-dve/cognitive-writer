#!/usr/bin/env python3
"""Rebuild research figures only from stored result JSON; never hand-enter measurements."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

LABELS = {"language":"语域与用词", "voice":"人称与语气", "rhythm":"句段节奏",
          "information":"信息揭示", "narration":"叙述与议论", "constraints":"条件与边界",
          "title":"标题", "opening":"开头", "ending":"结尾"}
ROOT = Path("research/style-aggregation")
plt.rcParams.update({"font.family":"Noto Sans SC", "axes.unicode_minus":False, "font.size":10})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    ap.add_argument("--pilot", action="store_true")
    args = ap.parse_args()
    run = ROOT / "runs" / args.run
    out = ROOT / "charts" / args.run
    out.mkdir(parents=True, exist_ok=True)
    summary = json.loads((run / "summary.json").read_text())
    if summary["status"] != "complete":
        raise ValueError("Cannot draw a complete experiment from an incomplete run")
    sources = [run / "summary.json"]
    if args.pilot:
        rows = summary["rows"]
        ids = [r.get("artifact_id", r["article_id"]) for r in rows]
        values = [r["characters"] for r in rows]
        with (out / "length.csv").open("w", newline="") as f:
            w = csv.writer(f); w.writerow(["article_id","non_whitespace_characters"]); w.writerows(zip(ids,values))
        fig, ax = plt.subplots(figsize=(max(8,len(rows)*.55),4.5))
        bars = ax.bar(ids, values, color=["#297D94" if i.startswith("wxb") else "#C97845" for i in ids])
        ax.bar_label(bars, padding=3); ax.axhline(1500, color="#777777", linestyle="--", label="完整度参考下限1500字")
        ax.set_ylabel("非空白字符数"); ax.set_title(f"{len(rows)}份风格说明：长度不是风格质量分")
        if len(rows)>8: ax.tick_params(axis="x",labelrotation=60)
        ax.set_ylim(0,max(values)*1.2); ax.legend(frameon=False)
        fig.tight_layout(); fig.savefig(out/"style-length.png",dpi=170); fig.savefig(out/"style-length.svg"); plt.close(fig)
    else:
        pairs = []
        for p in sorted((run / "pairs").glob("*.json")):
            sources.append(p); pairs.append(json.loads(p.read_text()))
        candidate_mode = all(isinstance(p["metrics"], dict) for p in pairs)
        assert candidate_mode or all(isinstance(p["metrics"], list) for p in pairs), "Mixed metric versions"
        if candidate_mode:
            for p in pairs:
                assert p["metrics"]["metric_version"] == "candidate_maximum_overlap_v2"
                p["metrics"] = p["metrics"]["overlap_only"]
        main_pairs = [p for p in pairs if p["kind"] == "main"]
        if len(main_pairs) != 190:
            raise ValueError("Expected all 190 distinct main pairs")
        ids = sorted({p[k] for p in main_pairs for k in ("left","right")})
        if len(ids) != 20 or len({tuple(sorted((p["left"],p["right"]))) for p in main_pairs}) != 190:
            raise ValueError("Duplicate or missing pairs")
        flat = []
        for p in pairs:
            for m in p["metrics"]:
                flat.append({"pair_id":p["id"],"kind":p["kind"],"left":p["left"],"right":p["right"],
                             "within_author":p["within_author"],**m})
        with (out/"pair-metrics.csv").open("w",newline="") as f:
            w=csv.DictWriter(f,fieldnames=list(flat[0])); w.writeheader(); w.writerows(flat)
        fig, axes = plt.subplots(3,3,figsize=(16,15))
        cmap=plt.colormaps["YlGnBu"].copy(); cmap.set_bad("#EEEEEE")
        for ax,(dim,label) in zip(axes.flat,LABELS.items()):
            matrix=np.full((20,20),np.nan)
            for p in main_pairs:
                metric=next(m for m in p["metrics"] if m["dimension"]==dim)
                i,j=ids.index(p["left"]),ids.index(p["right"])
                if metric["overlap"] is not None:
                    matrix[i,j]=matrix[j,i]=metric["overlap"]
            # Diagonal is deliberately blank: self-comparison is not an observed main pair.
            im=ax.imshow(matrix,vmin=0,vmax=1,cmap=cmap)
            ax.axhline(9.5,color="#A94733",linewidth=1); ax.axvline(9.5,color="#A94733",linewidth=1)
            ax.set_title(label); ax.set_xticks(range(20),ids,rotation=90,fontsize=6); ax.set_yticks(range(20),ids,fontsize=6)
        fig.suptitle("Style逐维表征重合：同作者区块与跨作者区块\n灰色为缺失或对角；不是生成保真度",fontsize=15)
        fig.colorbar(im,ax=list(axes.flat),shrink=.5,label="一对一特征重合率")
        fig.subplots_adjust(top=.92,bottom=.07,left=.07,right=.85,hspace=.45,wspace=.3)
        fig.savefig(out/"representation-heatmaps.png",dpi=170); plt.close(fig)
        fig,axes=plt.subplots(3,3,figsize=(13,11)); rng=np.random.default_rng(17)
        distributions=[]
        for ax,(dim,label) in zip(axes.flat,LABELS.items()):
            for pos,within in enumerate((True,False)):
                vals=[m["overlap"] for p in main_pairs if p["within_author"]==within for m in p["metrics"]
                      if m["dimension"]==dim and m["coverage"]=="both" and m["overlap"] is not None]
                ax.scatter(pos+rng.uniform(-.15,.15,len(vals)),vals,s=12,alpha=.45,color=("#297D94" if within else "#C97845"))
                if vals: ax.plot([pos-.2,pos+.2],[np.mean(vals)]*2,color="black",linewidth=2)
                distributions.append({"dimension":dim,"within_author":within,"n_pairs":len(vals),"mean":float(np.mean(vals)) if vals else None})
            ax.set_xticks([0,1],["作者内","作者间"]); ax.set_ylim(-.05,1.05); ax.set_title(label)
        fig.suptitle("作者内与作者间：仅比较双方均有描述的维度\n点为配对，黑线为均值；配对并非独立样本",fontsize=14)
        fig.tight_layout(rect=(0,0,1,.93)); fig.savefig(out/"within-between.png",dpi=170); plt.close(fig)
        (out/"descriptive-summary.json").write_text(json.dumps(distributions,ensure_ascii=False,indent=2)+"\n")
    for svg in out.glob("*.svg"):
        svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines()) + "\n")
    (out/"provenance.json").write_text(json.dumps({"run_id":args.run,"matplotlib":matplotlib.__version__,
        "script_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "inputs":{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}},indent=2)+"\n")
    print(out)


if __name__ == "__main__":
    main()
