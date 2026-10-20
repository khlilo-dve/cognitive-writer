#!/usr/bin/env python3
"""Readable, reproducible method-development report. No model calls."""
import hashlib
import json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT=Path("research/style-aggregation")
RUNS=[("p2-candidates-pilot-01","24项一批，开启推理"),
      ("p2-candidates-pilot-02","24项一批，关闭推理"),
      ("p2-candidate-cases-01","逐对复杂规则诊断"),
      ("p2-candidates-pilot-03","8项一批，规则正文，开启推理")]

def main():
    records=[];sources={}
    for run,label in RUNS:
        path=ROOT/"runs"/run/"summary.json"
        s=json.loads(path.read_text());assert s["status"]!="running", "Wait for the bounded pilot to finish"
        controls_path=path.with_name("controls.json")
        checks=json.loads(controls_path.read_text())["rows"] if controls_path.exists() else s["rows"]
        rows=s.get("rows",[])
        records.append(dict(run=run,label=label,status=s["status"],checks_passed=sum(r.get("passed") is True for r in checks),
            checks_total=len(checks),completed=sum(r["status"]=="complete" for r in rows),failed=sum(r["status"]=="failed" for r in rows),
            deferred=sum(r["status"]=="deferred" for r in rows),spent=s["usage"]["run_occupied_cny"]))
        sources[str(path)]=hashlib.sha256(path.read_bytes()).hexdigest()
    ledger=json.loads((ROOT/"usage-ledger.json").read_text())
    settled=sum(x["actual_micro"] for x in ledger["entries"].values() if x["state"]=="settled")/1e6
    unknown=sum(x["reserved_micro"] for x in ledger["entries"].values() if x["state"]!="settled")/1e6
    cost=round(settled+unknown,6)
    out=ROOT/"reports/candidate-progress-2026-10-11";out.mkdir(parents=True,exist_ok=True)
    chart=ROOT/"charts/candidate-progress-2026-10-11";chart.mkdir(parents=True,exist_ok=True)
    plt.rcParams.update({"font.family":"Noto Sans SC","axes.unicode_minus":False})
    fig,ax=plt.subplots(figsize=(9,4.6));bars=ax.barh([r["label"] for r in records],[r["spent"] for r in records],color="#297D94")
    ax.bar_label(bars,fmt="%.3f元",padding=4);ax.set_xlim(0,max(r["spent"] for r in records)*1.25)
    ax.set_xlabel("本轮预算占用（元，按未缓存价估算；不是实付账单）")
    ax.set_title("方法验证的费用：失败也计入账本\n四轮样本与目的不同，费用不代表质量排名")
    ax.invert_yaxis();fig.tight_layout();fig.savefig(chart/"costs.png",dpi=170);plt.close(fig)
    lines=["# 阶段进展：比较方法的改进、失败与费用", "", "日期：2026-10-11。", "",
        "最初目标没有改变：检验多文章聚合是否更稳定、保留作者辨识度，并改善新主题写作。当前仍在检验测量工具，尚未得出聚合优劣结论。", "",
        "## 这次做了什么", "", "将模型的含义判断与程序的一对一配对分开；失败后缩小任务、补充控制、复核具体错误，全部保留旧数据。", "",
        "|试验|控制或开发预期一致|真实样本状态|预算占用|", "|---|---:|---|---:|"]
    for r in records:
        real="10项开发诊断，不是文章配对" if "cases" in r["run"] else f"{r['completed']}/6完成，{r['failed']}失败，{r['deferred']}暂缓；未启动的其余项不算完成"
        lines.append(f"|{r['label']}|{r['checks_passed']}/{r['checks_total']}|{real}|{r['spent']:.6f}元|")
    last=records[-1]
    lines += ["", "## 看到了什么", "",
        "1. 简单控制通过并不保证真实规则判断可靠。第二轮控制全部通过，但真实样本既有漏答，也有把部分重叠误作整条等价的情况。",
        "2. 逐对判断修正了三个明显误判，但一个结尾规则仍有解释分歧。开发预期不是人工黄金答案，不能为了显示通过而改掉旧预期。",
        "3. 一条规则的引用段落可能包含其他规则。最新版只把规则正文与条件送去比较，原始引用仍随本地产物保留。",
        f"4. 最新轮状态为{last['status']}，真实样本{last['completed']}/6完成。2026-10-11复核全部10项等价与3项冲突，并抽查12项不同机制；仍发现丢失限制条件、补入另一侧没有的要求等问题，因此不采用完整矩阵。该复核是实现者审查，不是人工黄金评分。", "",
        "## 费用", "",f"全研究累计占用 **{cost:.6f}元**，其中用量按未缓存单价估算{settled:.6f}元、未知预留{unknown:.6f}元。100元总额尚余{100-cost:.6f}元。没有购买套餐或修改密钥。", "",
        "## 下一步", "", "按用户授权自主改进并执行，无需重复确认。已转向原子特征小试：一条只表达一个写法机制，通用事实政策另记，加入同文重复对照；原详细风格说明保持不变。新小试上限3元，仍包含在100元总额中。完整主矩阵、聚合与生成质量仍未完成，不把工程进展当作研究完成。", "",
        "模型分层保持：困难判断使用Pro，既定大纲与正文使用Flash；计数、证据定位、配对、图表、预算由程序完成。", "",
        "明细见飞书实验总览及项目research/style-aggregation/下各run、method-revision文档、journal.md与usage-ledger.json。"]
    (out/"report.md").write_text("\n".join(lines)+"\n")
    (out/"summary.json").write_text(json.dumps(dict(records=records,occupied_cny=cost,settled_estimate=settled,uncertain=unknown,inputs=sources),ensure_ascii=False,indent=2)+"\n")
    print(out)

if __name__=="__main__":main()
