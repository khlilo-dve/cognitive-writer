#!/usr/bin/env python3
"""Offline identity, receipt, length and obvious-copy checks for exploratory drafts."""
import argparse
import csv
import hashlib
import json
import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "research/style-aggregation"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    args = ap.parse_args()
    assert Path(args.run).name == args.run
    run = ROOT / "runs" / args.run
    frozen = json.loads((run / "inputs.json").read_text())
    summary = json.loads((run / "summary.json").read_text())
    jobs = frozen["jobs"]
    assert len(jobs) == len({j["id"] for j in jobs}) == 60
    assert sorted(j["method"] for j in jobs).count("aggregate") == 12
    assert sum(j["method"] == "single" for j in jobs) == 36
    assert sum(j["method"] == "neutral" for j in jobs) == 12
    assert set(j["topic_id"] for j in jobs) == {"tools", "study", "repair"}
    assert len([j for j in jobs if j["phase"] == "reference"]) == 20
    assert len([j for j in jobs if j["phase"] == "confirmation"]) == 40
    receipts = []
    for p in sorted((run / "requests").glob("*.json")):
        c = json.loads(p.read_text())
        assert digest(json.dumps(c["receipt"], ensure_ascii=False, separators=(",", ":")).encode()) == c["receipt_sha256"]
        receipts.append(dict(file=p.name, sha256=digest(p.read_bytes()), model=c["receipt"]["model"], finish_reason=c["receipt"]["finish_reason"]))
    assert all(r["model"] == "mimo-v2.6-flash" for r in receipts)
    specs = json.loads((ROOT / "runs/pragmatic-profiles-recovered-01/semantic-review.json").read_text())
    assert frozen["exploratory_profile_review"] == specs
    rows = []
    for job in jobs:
        path = run / "articles" / (job["id"] + ".json")
        if not path.exists():
            continue
        a = json.loads(path.read_text())
        assert a["job"] == job and a["body"] == path.with_suffix(".md").read_text()
        count = len("".join(a["body"].split()))
        # A literal match flags only a possible copied phrase, not plagiarism or author style.
        rows.append(dict(id=job["id"], author=job["author"], method=job["method"], phase=job["phase"], topic=job["topic_id"],
                         characters=count, in_600_900=(600 <= count <= 900), body_sha256=digest(a["body"].encode()),
                         literal_source_like=bool(re.search(r"(一车粪|莎士比亚本人|昆明的雨|一只特立独行的猪)", a["body"]))))
    output = ROOT / "reports" / args.run
    output.mkdir(parents=True, exist_ok=True)
    with (output / "articles.csv").open("w", newline="") as f:
        if rows:
            writer = csv.DictWriter(f, fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
    grouped = defaultdict(list)
    for row in rows:
        grouped[(row["phase"], row["method"])].append(row)
    groups = [dict(phase=p,method=m,n=len(group),mean_characters=round(sum(r["characters"] for r in group)/len(group),1),
                   length_pass=sum(r["in_600_900"] for r in group),literal_flags=sum(r["literal_source_like"] for r in group)) for (p,m),group in sorted(grouped.items())]
    result = dict(run=args.run,status=summary["status"],frozen_jobs=len(jobs),saved_articles=len(rows),receipt_count=len(receipts),
                  receipt_manifest=receipts,groups=groups,limitations=["Length is deterministic. Literal phrase flags are only leads for review, not factual or style scores.",
                  "Do not compare methods as quality until the same task and author are paired and assessed independently."])
    (output / "audit.json").write_text(json.dumps(result,ensure_ascii=False,indent=2) + "\n")
    print(json.dumps({k:v for k,v in result.items() if k!="receipt_manifest"}, ensure_ascii=False))


if __name__ == "__main__":
    main()
