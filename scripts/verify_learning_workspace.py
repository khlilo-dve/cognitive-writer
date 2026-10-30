#!/usr/bin/env python3
"""Exercise the offline CLI with real frozen drafts; all mutable workspace files are temporary."""
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/cognitive-writer"
CORE = ROOT / "research/style-aggregation/runs/voice-learning-core-01"


def main():
    ledger = ROOT / "research/style-aggregation/usage-ledger.json"
    ledger_before = hashlib.sha256(ledger.read_bytes()).hexdigest()
    plan = json.loads((CORE / "inputs.json").read_text())
    jobs = {j["id"]: j for j in plan["jobs"]}
    env = dict(os.environ, API_KEY="fixture", BASE_URL="http://127.0.0.1:1", MODEL="offline-not-used")
    checks = []
    with tempfile.TemporaryDirectory(prefix="cw-learning-workspace-") as tmp:
        temp = Path(tmp)
        workspace = temp / "workspace"

        def call(*args, ok=True, root=workspace):
            command = [str(BIN), "style-draft", "--root", str(root), *map(str, args)]
            result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True, timeout=20)
            assert (result.returncode == 0) == ok, (args, result.stderr)
            return result

        def bundle(ident, note, suffix="", root=temp):
            saved = json.loads((CORE / "profiles" / f"{ident}.json").read_text())
            value = {"articles": list(jobs[ident]["texts"]), "draft": saved["result"],
                     "origin": "model_output", "note": note, "review": None}
            path = root / f"{ident}{suffix}.json"
            path.write_text(json.dumps(value, ensure_ascii=False))
            return path, value

        def ident(result):
            return re.search(r"^candidate_id: ([0-9a-f]{64})$", result.stdout, re.M).group(1)

        final_ids = {}
        for code, author in (("wxb", "王小波"), ("wzq", "汪曾祺")):
            name = f"工作区验证-{author}"
            old_count = 0
            for version, count in enumerate((1, 3, 10), 1):
                path, value = bundle(f"{code}-k{count}", f"保留已有来源，查看{count}篇候选的变化")
                base = ["--base", name, version - 1] if version > 1 else []
                imported = call("import", path, *base)
                candidate = ident(imported)
                assert value["draft"]["spec"] in call("preview", candidate).stdout
                assert ident(call("import", path, *base)) == candidate
                changes = call("diff", candidate).stdout
                assert f"正文：{old_count} → {count}篇；新增{count-old_count}篇" in changes
                assert f"version: {version}" in call("save", candidate, name).stdout
                assert value["draft"]["spec"] in call("show", name, version).stdout
                old_count = count
                final_ids[code] = candidate
            history = call("versions", name).stdout
            assert all(f"第{n}版" in history for n in (1, 2, 3))
            output = temp / f"{code}-writer.md"
            call("export", name, 1, output)
            expected = json.loads((CORE / "profiles" / f"{code}-k1.json").read_text())["result"]["spec"]
            assert output.read_bytes() == expected.encode()
            call("export", name, 3, output, ok=False)
            assert output.read_bytes() == expected.encode()
        checks.append("both_authors_1_3_10_preview_restart_save_history_diff_and_spec_only_export")
        assert all(x in call("candidates").stdout for x in final_ids.values())
        assert "工作区验证-王小波" in call("list").stdout

        # Invalid imports do not add candidate files or change existing versions.
        candidate_count = len(list((workspace / "candidates").glob("*.json")))
        path, value = bundle("wxb-k1", "不能删除既有来源", "-reduced")
        call("import", path, "--base", "工作区验证-王小波", 3, ok=False)
        value["articles"].append(value["articles"][0])
        path.write_text(json.dumps(value, ensure_ascii=False))
        call("import", path, ok=False)
        value["articles"] = jobs["wxb-k1"]["texts"] * 11
        path.write_text(json.dumps(value, ensure_ascii=False))
        call("import", path, ok=False)
        value["articles"] = [jobs["wxb-k1"]["texts"][0] + "被改动的正文"]
        path.write_text(json.dumps(value, ensure_ascii=False))
        call("import", path, ok=False)
        assert len(list((workspace / "candidates").glob("*.json"))) == candidate_count
        call("preview", "../escape", ok=False)
        call("save", final_ids["wxb"], "../escape", ok=False)
        checks.append("reject_source_removal_duplicates_over10_mismatch_and_path_escape")

        # Concurrent saves from one base yield one new version, and retain the losing candidate.
        initial_path, _ = bundle("wxb-k1", "并发验证初版", "-race-initial")
        initial = ident(call("import", initial_path))
        call("save", initial, "并发验证")
        race_ids = []
        for tag in ("甲", "乙"):
            path, _ = bundle("wxb-k3", f"并发候选{tag}", f"-race-{tag}")
            race_ids.append(ident(call("import", path, "--base", "并发验证", 1)))
        processes = [subprocess.Popen([str(BIN), "style-draft", "--root", str(workspace),
                     "save", cid, "并发验证"], cwd=ROOT, env=env, text=True,
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE) for cid in race_ids]
        results = [p.communicate(timeout=20) for p in processes]
        assert sorted(p.returncode for p in processes) == [0, 1], results
        history = call("versions", "并发验证").stdout
        assert "第2版" in history and "第3版" not in history
        for cid in race_ids:
            assert cid in call("preview", cid).stdout
            call("save", cid, "并发验证", ok=False)
        call("save", initial, "并发验证", ok=False)
        checks.append("concurrent_stale_and_duplicate_name_saves_never_overwrite")

        # Orphan staging debris is ignored, but corruption of a published record is rejected.
        files = list((workspace / "versions").glob("*/v000001.json"))
        version_file = files[0]
        debris = version_file.parent / ".learning-interrupted.pending"
        debris.write_text("not a complete version")
        call("list")
        original = version_file.read_bytes()
        version_file.write_text("{}")
        call("list", ok=False)
        version_file.write_bytes(original)
        cid = final_ids["wxb"]
        candidate_file = workspace / "candidates" / f"{cid}.json"
        original = candidate_file.read_bytes()
        changed = json.loads(original)
        changed["input"]["note"] = "没有更新身份的篡改"
        candidate_file.write_text(json.dumps(changed, ensure_ascii=False))
        call("preview", cid, ok=False)
        call("show", "工作区验证-王小波", 3)
        candidate_file.write_bytes(original)
        checks.append("ignore_unpublished_pending_files_reject_corruption_and_keep_saved_snapshot")

        blocked = temp / "blocked-workspace"
        staged = ident(call("import", initial_path, root=blocked))
        (blocked / "versions").write_text("cannot create a version directory")
        call("save", staged, "失败重试", ok=False, root=blocked)
        assert staged in call("preview", staged, root=blocked).stdout
        checks.append("failed_save_retains_candidate_for_recovery")
    assert not temp.exists()
    ledger_after = hashlib.sha256(ledger.read_bytes()).hexdigest()
    assert ledger_before == ledger_after
    print(json.dumps({"status": "passed", "checks": checks,
        "frozen_drafts": 6, "named_version_chains": 2, "source_counts": [1, 3, 10],
        "temporary_workspace_cleaned": True, "new_model_requests": 0,
        "ledger_before_sha256": ledger_before, "ledger_after_sha256": ledger_after},
        ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
