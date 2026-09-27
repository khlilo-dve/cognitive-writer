#!/usr/bin/env python3
"""Fetch explicitly selected prose, preserving raw bytes and exact extraction boundaries.
Uses the environment's requests/BeautifulSoup; never generates or rewrites prose.
"""
import argparse
import hashlib
import json
import re
from datetime import datetime, timezone
from pathlib import Path
import requests
from bs4 import BeautifulSoup


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection", type=Path)
    ap.add_argument("output", type=Path)
    ap.add_argument("--inspect", action="store_true")
    args = ap.parse_args()
    selection = json.loads(args.selection.read_text())
    root = args.output
    root.mkdir(parents=True, exist_ok=True)
    (root / "sources").mkdir(exist_ok=True)
    (root / "raw").mkdir(exist_ok=True)
    articles = []
    for item in selection["articles"]:
        ident = item["article_id"]
        raw_path = root / "raw" / f"{ident}.html"
        if raw_path.exists():
            raw = raw_path.read_bytes()
        else:
            r = requests.get(item["source_url"], timeout=45)
            r.raise_for_status()
            raw = r.content
            raw_path.write_bytes(raw)
        soup = BeautifulSoup(raw.decode(item.get("encoding", "utf-8")), "html.parser")
        node = (max(soup.find_all("td"), key=lambda x: len(x.get_text()))
                if item["selector"] == "largest_td" else soup.select_one(item["selector"]))
        if node is None:
            raise ValueError(f"{ident}: missing selected body")
        paragraphs = [p.get_text("", strip=True) for p in node.find_all("p")]
        text = "\n\n".join(p for p in paragraphs if p)
        if not text:
            text = "\n\n".join(node.stripped_strings)
        if "start" in item:
            start, end = item["start"], item["end"]
            if text.count(start) != 1 or text.count(end) != 1:
                raise ValueError(f"{ident}: ambiguous/missing boundary")
            text = text[text.index(start):text.index(end) + len(end)]
        # Publication/signature dates are metadata, not author style features.
        text = re.sub(r"(?:\n\s*)+[0-9一二三四五六七八九〇零十]{4}年[^\n]*$", "", text.strip())
        body = text.replace("\r\n", "\n").replace("\r", "\n").strip() + "\n"
        if "\ufffd" in body:
            raise ValueError(f"{ident}: invalid decoding")
        out = root / "sources" / f"{ident}.md"
        if out.exists() and out.read_text() != body and not args.inspect:
            raise ValueError(f"{ident}: existing source drift; use new dataset_id")
        out.write_text(body, encoding="utf-8")
        articles.append({**item, "path": f"sources/{ident}.md", "sha256": sha(body.encode()),
                         "raw_sha256": sha(raw), "characters": len("".join(body.split())),
                         "verified": item.get("verified", False)})
        print(ident, item["title"], len("".join(body.split())), "verified=" + str(item.get("verified", False)),
              "[held out]" if item["split"] != "discovery" else repr(body[:65]) + " ... " + repr(body[-65:]))
    manifest = {"schema": 1, "dataset_id": selection["dataset_id"], "articles": articles}
    path = root / ("inspection.json" if args.inspect else "manifest.json")
    if path.exists() and json.loads(path.read_text()) != manifest and not args.inspect:
        raise ValueError("manifest drift: use a new dataset id")
    path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    provenance = root / ("acquisition-inspection.json" if args.inspect else "acquisition.json")
    if not provenance.exists():
        provenance.write_text(json.dumps({"acquired_at": datetime.now(timezone.utc).isoformat(),
            "selection_sha256": sha(args.selection.read_bytes()), "script_sha256": sha(Path(__file__).read_bytes()),
            "cleaning": "HTML p tags; explicit start/end boundaries; no content rewriting"}, indent=2))


if __name__ == "__main__":
    main()
