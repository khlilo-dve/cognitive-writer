#!/usr/bin/env python3
"""Prepare exact-offset ending excerpts; punctuation segments are not semantic labels."""
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"


def anchors(text):
    segments = []
    for match in re.finditer(r"[^。！？!?]+[。！？!?]?", text):
        raw = match.group()
        quote = raw.strip()
        if not any(c.isalnum() for c in quote):
            continue
        start = match.start() + len(raw) - len(raw.lstrip())
        end = start + len(quote)
        assert text[start:end] == quote
        segments.append(dict(start_character=start, end_character=end, quote=quote))
    lines = [(i+1, line) for i, line in enumerate(text.split("\n")) if line.strip()]
    return dict(last_nonempty_line=lines[-1][0], last_nonempty_line_text=lines[-1][1],
                final_punctuation_segments=segments[-2:])


def main():
    source = ROOT / "runs/p3-evidence-audit-01/inputs.json"
    frozen = json.loads(source.read_text())
    rows = []
    for article, (_, data, _) in frozen["sources"].items():
        rows.append(dict(article=article, source_sha256=hashlib.sha256(data["text"].encode()).hexdigest(), **anchors(data["text"])))
    out = ROOT / "reports/evidence-anchors-v1.json"
    out.write_text(json.dumps(dict(input_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        offset_unit="zero-based Unicode characters, half-open", scope="20 discovery texts only",
        limitation="标点切片帮助定位，不自动判断文学结尾、反讽、升华或作者稳定性；不能把位置正确当语义支持。", rows=rows), ensure_ascii=False, indent=2)+"\n")
    print(f"Saved {len(rows)} source-verified ending anchors; zero model calls")


if __name__ == "__main__":
    main()
