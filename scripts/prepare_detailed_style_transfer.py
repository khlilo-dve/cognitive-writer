#!/usr/bin/env python3
"""Freeze the 12-job style-transfer pilot before seeing generated outputs."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"
RUN = ROOT / "runs/detailed-transfer-pilot-01"
DATA = ROOT / "datasets"
PROFILES = ROOT / "runs/detailed-executable-style-01/profiles"
SINGLE = ROOT / "runs/p2-detailed-single-01/single"
PROFILE = {"wang_xiaobo": "wang-xiaobo.md", "wang_zengqi": "wang-zengqi.md"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    manifest_raw = (DATA / "authors-v1/manifest.json").read_bytes()
    manifest = json.loads(manifest_raw)
    sampling = json.loads((DATA / "sampling-v1.json").read_text())
    tasks_raw = (DATA / "detailed-transfer-tasks-v1.json").read_bytes()
    tasks = json.loads(tasks_raw)
    assert sampling["manifest_sha256"] == digest(manifest_raw)
    assert tasks["suite"] == "detailed_style_transfer_pilot_v1"
    assert [t["id"] for t in tasks["tasks"]] == ["lost_cards", "seed_box"]
    assert "600" not in tasks["instruction"] and "900" not in tasks["instruction"]
    sources = {a["article_id"]: a for a in manifest["articles"] if a["split"] == "discovery"}
    assert len(sources) == 20
    jobs = []
    file_hashes = {"manifest": digest(manifest_raw), "tasks": digest(tasks_raw)}
    for author in PROFILE:
        profile_path = PROFILES / PROFILE[author]
        spec = profile_path.read_text()
        assert spec.startswith("# 写作声音说明\n")
        assert "王小波" not in spec and "汪曾祺" not in spec
        file_hashes[PROFILE[author]] = digest(profile_path.read_bytes())
        profile_sources = sorted(aid for aid, source in sources.items() if source["author_id"] == author)
        assert len(profile_sources) == 10
        for task in tasks["tasks"]:
            baseline = sampling["authors"][author]["generation_baselines"][task["baseline_topic"]]
            assert sources[baseline]["author_id"] == author
            single_path = SINGLE / f"{baseline}.md"
            single_json = json.loads((SINGLE / f"{baseline}.json").read_text())
            single = single_path.read_text()
            assert single_json["result"]["spec"] == single
            assert "王小波" not in single and "汪曾祺" not in single
            file_hashes[f"single/{baseline}"] = digest(single_path.read_bytes())
            task_text = (tasks["instruction"] + "\n主题：" + task["title"] + "\n核心素材：" + task["facts"]
                         + "\n必须覆盖：" + task["requirements"])
            for method, style_id, source_articles, style in (
                ("aggregate", f"detailed-{author}-v1", profile_sources, spec),
                ("single", baseline, [baseline], single),
                ("neutral", "none", [], "未指定特定作者风格。根据素材写自然、清楚的中文随笔。"),
            ):
                jobs.append({"id": f"article-{len(jobs)+1:03}", "author": author, "method": method,
                             "style_id": style_id, "source_articles": source_articles, "topic_id": task["id"],
                             "phase": "development", "replicate": 1, "spec": style, "task": task_text})
    assert len(jobs) == 12
    plan = {"schema": 1, "suite": "detailed_style_transfer_pilot_v1", "method": "method-revision-17",
            "writer": "mimo-v2.6-flash", "run_cap_micro_cny": 1_000_000,
            "file_sha256": file_hashes, "jobs": jobs}
    RUN.mkdir(parents=True, exist_ok=True)
    target = RUN / "inputs.json"
    content = (json.dumps(plan, ensure_ascii=False, indent=2) + "\n").encode()
    if target.exists():
        assert target.read_bytes() == content, "Frozen plan changed; make a new run"
    else:
        target.write_bytes(content)
    print(json.dumps({"run": RUN.name, "jobs": len(jobs), "plan_sha256": digest(content)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
