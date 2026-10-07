#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT=Path("research/style-aggregation")
source=ROOT/"reports/P2-comparison-diagnosis/candidate-diagnostic.json"
summary=json.loads(source.read_text())["summary"]
labels=["可解析为JSON", "包含跨维度匹配", "重复使用匹配端点"]
values=[summary["parseable"],summary["responses_with_cross_dimension_matches"],summary["responses_with_repeated_match_endpoints"]]
plt.rcParams.update({"font.family":"Noto Sans SC","axes.unicode_minus":False})
fig,ax=plt.subplots(figsize=(9,4.4))
bars=ax.barh(labels,values,color=["#368C9C","#D89243","#BA5652"])
ax.invert_yaxis();ax.bar_label(bars,padding=6);ax.set_xlim(0,summary["response_count"]*1.15)
ax.set_xlabel("响应份数");ax.set_title(f"{summary['response_count']}份Pro首轮响应：JSON合法不等于比较可用",pad=16)
fig.text(.04,.02,"两类问题可以重叠。仅统计首轮等价匹配声明，不含后续修复及冲突列表。",fontsize=10,color="#555555")
fig.tight_layout(rect=(0,.08,1,1))
out=ROOT/"charts/p2-comparison-stop";out.mkdir(parents=True,exist_ok=True)
fig.savefig(out/"structure-issues.png",dpi=180);plt.close(fig)
(out/"provenance.json").write_text(json.dumps(dict(source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),values=dict(zip(labels,values))),ensure_ascii=False,indent=2)+"\n")
print(out/"structure-issues.png")
