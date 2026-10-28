#!/usr/bin/env python3
"""Publish single/multi draft examples and the honest scope-revision readout."""
import json
import re
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from publish_style_progress import call
from publish_style_stop_report import publish

ROOT = Path("research/style-aggregation")


def verified(key, title, text, tail):
    doc = publish(key, title, text)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    markdown = fetched["data"]["markdown"]
    assert "待审" in markdown and tail in markdown
    for heading in re.findall(r"^#{2,4}\s+(.+)$", text, flags=re.M):
        assert heading.replace("**", "") in markdown.replace("**", ""), heading
    (ROOT / "lark" / f"{key}-fetch.json").write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    return doc["data"]["doc_url"]


def main():
    links = []
    for code, author in (("wxb", "王小波"), ("wzq", "汪曾祺")):
        for count in (1, 10):
            ident = f"{code}-k{count}"
            spec = (ROOT / f"runs/voice-learning-scope-reviewed-01/profiles/{ident}.md").read_text()
            text = ("**待审开发草稿**。模型原文复读后，经实现者有限定点修改；不属于独立人评或全自动方法验收。"
                    "本轮18稿诊断使用的是这些定点修改之前的模型稿，仍出现Writer事实越界，"
                    "因此不能把本文件当作已通过新题材验证的正式风格。原稿、修改与理由都保留。\n\n---\n\n" + spec)
            tail = next(line.strip() for line in reversed(spec.splitlines()) if line.strip())
            # A short suffix checks the end as well as every section heading.
            tail = tail.replace("**", "")[-18:]
            url = verified(f"voice-scope-{ident}-20261029", f"Cognitive Writer｜{author}{count}篇风格说明（待审修订稿）", text, tail)
            links.append(f"- [{author}{count}篇风格说明（待审修订稿）]({url})")
    report = (ROOT / "reports/voice-scope-2026-10-29.md").read_text()
    report += "\n\n## 本轮实际说明文件（待审）\n\n" + "\n".join(links) + "\n"
    url = verified("voice-scope-stage-20261029", "Cognitive Writer｜原文复读、18稿诊断与下一步", report,
                   links[-1].split("](")[1][:-1])
    print(json.dumps({"report_url": url, "drafts": links, "verified": True}, ensure_ascii=False))


if __name__ == "__main__":
    main()
