#!/usr/bin/env python3
"""Freeze metadata-only research subsets before inspecting comparative outcomes."""
import hashlib
import json
import random
from pathlib import Path

ROOT = Path("research/style-aggregation")
SEEDS = [17, 29, 43, 71, 101]
SIZES = [1, 2, 3, 5, 7, 10]


def main():
    manifest = ROOT / "datasets/authors-v1/manifest.json"
    articles = [a for a in json.loads(manifest.read_text())["articles"] if a["split"] == "discovery"]
    result = {"algorithm": "topic_round_robin_python_random_v1", "seeds": SEEDS,
              "sizes": SIZES, "manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
              "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "authors": {}}
    for author in sorted({a["author_id"] for a in articles}):
        group = [a for a in articles if a["author_id"] == author]
        topics = sorted({a["topic"] for a in group})
        assert len(group) == 10 and len(topics) == 3
        by_topic = {t: sorted(a["article_id"] for a in group if a["topic"] == t) for t in topics}
        baseline_rng = random.Random(17)
        entry = {"generation_baselines": {t: baseline_rng.choice(by_topic[t]) for t in topics}, "orders": []}
        for seed in SEEDS:
            rng = random.Random(seed)
            pools = {t: ids.copy() for t, ids in by_topic.items()}
            for ids in pools.values():
                rng.shuffle(ids)
            topic_order = topics.copy()
            rng.shuffle(topic_order)
            order = []
            while any(pools.values()):
                for t in topic_order:
                    if pools[t]:
                        order.append(pools[t].pop())
            assert len(order) == len(set(order)) == 10
            assert set(order) == {a["article_id"] for a in group}
            entry["orders"].append({"seed": seed, "order": order,
                                    "subsets": {str(k): sorted(order[:k]) for k in SIZES}})
        result["authors"][author] = entry
    out = ROOT / "datasets/sampling-v1.json"
    if out.exists() and json.loads(out.read_text()) != result:
        raise ValueError("Frozen sampling changed; do not overwrite")
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print("Frozen 10 orders and 6 generation baselines; no outcome data read")


if __name__ == "__main__":
    main()
