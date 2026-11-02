#!/usr/bin/env python3
"""Publish method29 report and immutable model bundles, then verify user-owned Docs."""
import json
import sys
from pathlib import Path
sys.dont_write_bytecode = True
from publish_style_stop_report import publish
from publish_style_progress import call

ROOT = Path("research/style-aggregation")


def publish_checked(key, title, path, markers):
    content = path.read_text().split("\n", 1)[1]
    doc = publish(key, title, content)
    fetched = call("+fetch", doc=doc["data"]["doc_id"])
    encoded = json.dumps(fetched, ensure_ascii=False)
    assert not fetched.get("data", {}).get("has_more"), "Need pagination before claiming full readback"
    for marker in markers:
        assert marker in encoded, f"Missing readback marker: {marker}"
    (ROOT / "lark" / (key + "-fetch.json")).write_text(
        json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    print("Verified user-owned document:", doc["data"]["doc_url"], flush=True)
    return doc["data"]["doc_url"]


def main():
    style_url = publish_checked("learning-grounded-styles-20261102",
        "Cognitive Writer｜方法29四份自动风格说明（待审）",
        ROOT / "reports/learning-grounded-styles-2026-11-02.md",
        ["王小波｜1篇", "王小波｜2篇", "汪曾祺｜1篇", "汪曾祺｜2篇", "不设配额"])
    draft_url = publish_checked("learning-grounded-drafts-20261102",
        "Cognitive Writer｜四篇Flash边界试写原稿（含事实问题）",
        ROOT / "reports/learning-grounded-drafts-2026-11-02.md",
        ["wxb-k1-abstract_choices", "wxb-k2-abstract_choices", "wzq-k1-given_feeling", "wzq-k2-given_feeling", "不確定以後"])
    report = ROOT / "reports/learning-grounded-2026-11-02.md"
    content = report.read_text().replace("{{STYLE_DOC_URL}}", style_url).replace("{{DRAFT_DOC_URL}}", draft_url)
    report.write_text(content)
    publish_checked("learning-grounded-stage-20261102",
        "Cognitive Writer｜风格学习阶段报告：来源核验与四稿边界检查",
        report, ["24.556135", "1.820037", style_url, draft_url, "不静默切换正式默认入口"])


if __name__ == "__main__":
    main()
