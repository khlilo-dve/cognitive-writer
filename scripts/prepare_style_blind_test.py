#!/usr/bin/env python3
"""Freeze four preregistered A/B comparisons without inspecting draft quality."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"
DATA = ROOT / "datasets"
RUN = ROOT / "runs/pragmatic-generation-01"
OUT = ROOT / "blind-test/ab-v1"
PAIRS = [
    ("一", "wang_xiaobo", "tools", "aggregate"),
    ("二", "wang_xiaobo", "repair", "single"),
    ("三", "wang_zengqi", "tools", "single"),
    ("四", "wang_zengqi", "repair", "aggregate"),
]
REFERENCE = {"wang_xiaobo": "wxb-07", "wang_zengqi": "wzq-07"}
BASELINE_TOPIC = {"tools": "daily", "repair": "society"}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def write_once(path, content):
    if path.exists():
        assert path.read_bytes() == content, f"Frozen output changed: {path}"
    else:
        path.write_bytes(content)


def main():
    inputs = json.loads((RUN / "inputs.json").read_text())
    summary = json.loads((RUN / "summary.json").read_text())
    sampling = json.loads((DATA / "sampling-v1.json").read_text())
    manifest = json.loads((DATA / "authors-v1/manifest.json").read_text())
    assert summary["status"] == "complete" and len(inputs["jobs"]) == 60
    articles = {x["article_id"]: x for x in manifest["articles"]}
    candidates = {}
    for job in inputs["jobs"]:
        if job["replicate"] == 1 and job["method"] in ("aggregate", "single"):
            candidates.setdefault((job["author"], job["topic_id"], job["method"]), []).append(job)

    lines = [
        "# 写作声音 A/B 小问卷｜阅读材料", "",
        "请先读两位匿名作者的原文短片段，再读四组同题稿件。每组 A/B 的写作任务和事实素材相同；请只根据读到的文字判断。",
        "短片段只是局部参照。正文中的事实设定均为写作实验，不是真实新闻。请勿搜索作者或讨论答案；问卷不收集姓名。", "",
    ]
    mapping = {"schema": 1, "run": "pragmatic-generation-01", "protocol": "method-revision-16", "references": {}, "pairs": []}
    for author, label in (("wang_xiaobo", "作者一"), ("wang_zengqi", "作者二")):
        ref = articles[REFERENCE[author]]
        assert ref["author_id"] == author and ref["split"] == "discovery"
        raw = (DATA / "authors-v1" / ref["path"]).read_bytes()
        assert sha(raw) == ref["sha256"]
        excerpt = raw.decode().split("\n\n", 1)[0][:180].strip() + "……"
        assert "王小波" not in excerpt and "汪曾祺" not in excerpt
        mapping["references"][label] = {"article_id": ref["article_id"], "source_sha256": ref["sha256"], "excerpt_sha256": sha(excerpt.encode())}
        lines += [f"## {label}原文短片段", "", f"> {excerpt}", ""]

    task_defs = {x["id"]: x for x in json.loads((DATA / "generation-tasks-v1.json").read_text())["tasks"]}
    for numeral, author, topic, first in PAIRS:
        role = "作者一" if author == "wang_xiaobo" else "作者二"
        single_source = sampling["authors"][author]["generation_baselines"][BASELINE_TOPIC[topic]]
        selected = {}
        for method in ("aggregate", "single"):
            options = candidates[(author, topic, method)]
            if method == "single":
                options = [j for j in options if j["source_articles"] == [single_source]]
            assert len(options) == 1, (author, topic, method, len(options))
            job = options[0]
            article = json.loads((RUN / "articles" / (job["id"] + ".json")).read_text())
            assert article["job"] == job
            body = article["body"]
            assert body == (RUN / "articles" / (job["id"] + ".md")).read_text()
            assert "王小波" not in body and "汪曾祺" not in body
            selected[method] = (job, body)
        second = "single" if first == "aggregate" else "aggregate"
        task = task_defs[topic]
        lines += [f"## 第{numeral}组｜{role}｜{task['title']}", "", f"**共同素材：**{task['facts']}", "",
                  f"**共同写作要求：**{task['requirements']}", ""]
        pair = {"label": numeral, "author": author, "topic": topic, "phase": task["phase"], "sides": {}}
        for side, method in (("A", first), ("B", second)):
            job, body = selected[method]
            pair["sides"][side] = {"method": method, "article_id": job["id"], "style_id": job["style_id"], "body_sha256": sha(body.encode())}
            lines += [f"### {side}稿", "", body.rstrip(), ""]
        lines += ["---", ""]
        mapping["pairs"].append(pair)
    lines += ["请在问卷中对每组分别选择：哪篇更接近对应作者短片段的写法；哪篇读起来更自然。两项可以选择不同答案。若发现具体事实错误，请在末尾写明组别与句子。", ""]
    OUT.mkdir(parents=True, exist_ok=True)
    packet = ("\n".join(lines).rstrip() + "\n").encode()
    mapping["packet_sha256"] = sha(packet)
    write_once(OUT / "packet.md", packet)
    write_once(OUT / "mapping.json", (json.dumps(mapping, ensure_ascii=False, indent=2) + "\n").encode())
    print(json.dumps({"pairs": len(mapping["pairs"]), "packet_sha256": mapping["packet_sha256"], "ids": [p["sides"] for p in mapping["pairs"]]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
