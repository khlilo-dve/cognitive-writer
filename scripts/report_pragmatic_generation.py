#!/usr/bin/env python3
"""Readable deterministic report for the frozen 60-draft exploratory generation."""
import json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT=Path("research/style-aggregation")
RUN="pragmatic-generation-01"
LABELS={"aggregate":"多篇聚合","single":"单篇风格","neutral":"无风格"}


def main():
    directory=ROOT/"reports"/RUN
    audit=json.loads((directory/"audit.json").read_text())
    summary=json.loads((ROOT/"runs"/RUN/"summary.json").read_text())
    assert summary["status"]=="complete" and audit["saved_articles"]==60 and audit["receipt_count"]==120
    rows={(r["phase"],r["method"]):r for r in audit["groups"]}
    assert len(rows)==6
    plt.rcParams.update({"font.family":"Noto Sans SC","axes.unicode_minus":False})
    fig,axes=plt.subplots(1,2,figsize=(10,4.5),layout="constrained",sharey=True)
    for ax,(phase,title) in zip(axes,[("reference","开发题目"),("confirmation","留出题目")]):
        values=[rows[(phase,m)]["length_pass"]/rows[(phase,m)]["n"] for m in LABELS]
        bars=ax.bar(list(LABELS.values()),values,color=["#396f92","#b78555","#89979f"])
        for bar,m in zip(bars,LABELS):
            group=rows[(phase,m)]
            ax.annotate(f"{group['length_pass']}/{group['n']}",(bar.get_x()+bar.get_width()/2,bar.get_height()),
                        textcoords="offset points",xytext=(0,4),ha="center")
        ax.set_title(title)
        ax.set_ylim(0,1.04)
        ax.set_ylabel("600–900非空白字符达标率")
        ax.spines[["top","right"]].set_visible(False)
    fig.suptitle("60篇完整生成：字数达标率（不是风格或内容质量）")
    chart=ROOT/"charts"/RUN;chart.mkdir(parents=True,exist_ok=True)
    fig.savefig(chart/"length-compliance.png",dpi=170)
    plt.close(fig)
    lines=["# P5探索性生成：60篇已完成", "", "从两位作者各10篇原文派生探索性风格候选；保留三份单篇Style基线与无风格组。同一Flash Writer、三项冻结素材，48篇主对照加12篇中性对照；所有120份大纲/正文响应有SHA、模型与费用记录。", "",
        "|阶段|方法|篇数|平均非空白字符|达到600–900字|", "|---|---|---:|---:|---:|"]
    for phase,title in [("reference","开发题目"),("confirmation","留出题目")]:
        for method,label in LABELS.items():
            group=rows[(phase,method)]
            lines.append(f"|{title}|{label}|{group['n']}|{group['mean_characters']:.1f}|{group['length_pass']}/{group['n']}|")
    lines += ["", "字数由程序统计，短稿不重抽。聚合Style仅1288/1348非空白字符，低于详细说明完整度参考；原严格聚合失败、4处引文不匹配及2条单题材机制均留证，结果不能代表已经学会稳定作者风格。", "",
        "实现者初步查到的事实风险例子：一篇维修稿加入素材未提供的拆后盖、接设备检查动作；一篇工具柜稿加入房号和借用单等操作细节。它们是待逐篇评分核验的具体线索，不是完整违规率。", "",
        f"本run预算占用{summary['usage']['run_occupied_cny']:.6f}元，全研究累计占用{summary['usage']['occupied_cny']:.6f}元（含未知预留，不是实付账单）。模型风格、事实、自然度与作者匹配尚待独立评估，不由字数推出优劣。",
        "", "原始60行与哈希校验在reports/pragmatic-generation-01/articles.csv、audit.json；图表在charts/pragmatic-generation-01/length-compliance.png。确认集原文直到既定确认评分阶段才会读取。"]
    (directory/"report.md").write_text("\n".join(lines)+"\n")
    print(directory/"report.md")


if __name__=="__main__":main()
