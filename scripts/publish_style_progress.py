#!/usr/bin/env python3
"""Publish the fixed P2 milestone report and verify its chart and cost disclosure."""
import json
import re
import subprocess
from pathlib import Path

ROOT = Path("research/style-aggregation")
CLI = ["/home/khlilo/.nvm/versions/node/v20.20.1/bin/node",
       "/home/khlilo/.nvm/versions/node/v20.20.1/bin/lark-cli", "docs"]


def call(command, **kwargs):
    argv = CLI + [command, "--as", "user"]
    for key, value in kwargs.items():
        argv += ["--" + key.replace("_", "-"), str(value)]
    result = subprocess.run(argv, capture_output=True, text=True, timeout=120)
    parsed = json.loads(result.stdout)
    if result.returncode or not parsed.get("ok"):
        raise RuntimeError(f"{command} failed; no automatic write retry")
    return parsed


def main():
    state = ROOT / "lark/p2-single-report-doc.json"
    if state.exists():
        doc = json.loads(state.read_text())
    else:
        content = (ROOT / "reports/P2-single-progress.md").read_text().split("\n", 1)[1]
        content = re.sub(r"!\[[^\]]*\]\([^)]*\)", "（长度图附在文末）", content)
        doc = call("+create", title="Cognitive Writer｜P2进展：20篇独立风格说明", markdown=content)
        state.write_text(json.dumps(doc, ensure_ascii=False, indent=2) + "\n")
    doc_id = doc["data"]["doc_id"]
    media_state = ROOT / "lark/p2-single-report-image.json"
    if not media_state.exists():
        media = call("+media-insert", doc=doc_id,
                     file=ROOT / "charts/p2-detailed-single-01/style-length.png",
                     caption="20份独立风格说明的长度；蓝色王小波，橙色汪曾祺。长度不是质量分。")
        media_state.write_text(json.dumps(media, ensure_ascii=False, indent=2) + "\n")
    fetched = call("+fetch", doc=doc_id)
    (ROOT / "lark/p2-single-report-fetch.json").write_text(json.dumps(fetched, ensure_ascii=False, indent=2) + "\n")
    encoded = json.dumps(fetched, ensure_ascii=False)
    assert "0.908841" in encoded and "0.143520" in encoded and "<image" in encoded
    assert "不能据此认定" in encoded
    print("Verified user-owned report and image:", doc["data"]["doc_url"])


if __name__ == "__main__":
    main()
