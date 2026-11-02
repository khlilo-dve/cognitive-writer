#!/usr/bin/env python3
"""Local HTTP/CLI verification of fresh learning, append and bounded resume. No paid calls."""
import hashlib
import http.server
import json
import os
import re
import shutil
import signal
import subprocess
import tempfile
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/cognitive-writer"
PROMPTS = ("learn_voice_analysis.md", "learn_voice_compile.md", "review_voice_scope.md", "revise_voice_scope.md")
SPEC = "# 说明\n\n依据给定材料组织判断，叙述与解释按任务交替，材料薄时不补事实。"


def main():
    ledger = ROOT / "research/style-aggregation/usage-ledger.json"
    before = hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests = []
    control = {"fail_compile_once": True, "hold": False}
    received = threading.Event()
    release = threading.Event()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(body)
            assert body["model"] == "mimo-v2.6-pro"
            assert body["thinking"] == {"type": "disabled"}
            assert body["max_completion_tokens"] == 16384
            assert "response_format" not in body
            system = body["messages"][0]["content"]
            if control["hold"]:
                received.set()
                release.wait(10)
            if "完整阅读输入的" in system:
                text = "内部分析专用标记：判断与材料按内容条件相接。"
            elif "把内部声音分析编译" in system:
                if control["fail_compile_once"]:
                    control["fail_compile_once"] = False
                    self.send_response(503)
                    self.end_headers()
                    self.wfile.write(b"PRIVATE_ERROR_BODY")
                    return
                text = f"```markdown\n{SPEC}\n```"
            elif "你是写法说明的原文复读者" in system:
                user = json.loads(body["messages"][1]["content"])
                assert set(user) == {"articles", "candidate"}
                assert "内部分析专用标记" not in body["messages"][1]["content"]
                text = "内部复读专用标记：说明只描述可见机制，不给手法设配额。"
            else:
                assert "完整原文" in system and "复读意见" in system
                user = json.loads(body["messages"][1]["content"])
                assert set(user) == {"article_count", "articles", "candidate", "source_review"}
                assert len(user["articles"]) == user["article_count"]
                assert {a["text"] for a in user["articles"]} <= {"SOURCE_A：第一篇独特正文，记录真实的材料甲。", "SOURCE_B：第二篇不同正文，记录另外的材料乙。"}
                assert "内部分析专用标记" not in body["messages"][1]["content"]
                text = f"```md\n{SPEC}\n```"
            payload = json.dumps({"model": "mimo-v2.6-pro", "choices": [
                {"message": {"content": text}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 100, "completion_tokens": 50}}).encode()
            try:
                self.send_response(200)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
            except BrokenPipeError:
                pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    checks = []
    try:
        with tempfile.TemporaryDirectory(prefix="cw-learning-jobs-") as tmp:
            temp = Path(tmp)
            (temp / "prompts").mkdir()
            for name in PROMPTS:
                shutil.copyfile(ROOT / "prompts" / name, temp / "prompts" / name)
            env = dict(os.environ, API_KEY="fixture", BASE_URL=f"http://127.0.0.1:{server.server_port}/v1",
                       MODEL="mimo-v2.6-flash")
            workspace = temp / "workspace"

            def argv(*args, root=workspace):
                return [str(BIN), "style-draft", "--root", str(root), *map(str, args)]

            def call(*args, ok=True, config=env, root=workspace):
                result = subprocess.run(argv(*args, root=root), cwd=temp, env=config,
                                        capture_output=True, text=True, timeout=15)
                assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
                assert "PRIVATE_ERROR_BODY" not in result.stdout + result.stderr
                return result

            def field(result, key):
                return re.search(rf"^{key}: ([0-9a-f]{{64}})$", result.stdout, re.M).group(1)

            a, b = "SOURCE_A：第一篇独特正文，记录真实的材料甲。", "SOURCE_B：第二篇不同正文，记录另外的材料乙。"
            source = temp / "texts.json"
            source.write_text(json.dumps({"articles": [a], "note": "新建学习任务"}, ensure_ascii=False))
            started = call("learn", source, ok=False)
            job = field(started, "job_id")
            assert len(requests) == 2
            assert not (workspace / "versions").exists()
            call("resume", job, ok=False)
            assert len(requests) == 2
            assert "响应未知的请求：1" in call("job", job).stdout
            # Resume uses the frozen source and prompts, not these changed files.
            source.write_text(json.dumps({"articles": ["替换后的输入，不能进入旧任务"], "note": "已修改"}, ensure_ascii=False))
            for name in PROMPTS:
                (temp / "prompts" / name).write_text("CHANGED_PROMPT_MUST_NOT_BE_USED")
            finished = call("resume", job, "--retry-uncertain")
            candidate = field(finished, "candidate_id")
            assert len(requests) == 5 and SPEC in finished.stdout
            assert all("CHANGED_PROMPT" not in str(request) for request in requests)
            assert "响应未知的请求：1" in call("job", job).stdout
            call("save", candidate, "本地学习链")
            call("resume", job, config=dict(env, API_KEY=""))
            assert len(requests) == 5
            checks.append("fresh_pro_learning_bounded_explicit_retry_frozen_inputs_and_keyless_completed_resume")

            # Append restores the original snapshots. The old Style is absent from analysis input.
            for name in PROMPTS:
                shutil.copyfile(ROOT / "prompts" / name, temp / "prompts" / name)
            added = temp / "added.json"
            added.write_text(json.dumps({"articles": [b], "note": "加入另一篇正文重新综合"}, ensure_ascii=False))
            appended = call("append", "本地学习链", 1, added)
            append_job = field(appended, "job_id")
            append_candidate = field(appended, "candidate_id")
            assert len(requests) == 9
            analysis_input = json.loads(requests[5]["messages"][1]["content"])
            assert set(analysis_input) == {"articles"}
            assert {x["text"] for x in analysis_input["articles"]} == {a, b}
            assert "正文：1 → 2篇；新增1篇" in call("diff", append_candidate).stdout
            assert "第2版" not in call("versions", "本地学习链").stdout
            call("save", append_candidate, "本地学习链")
            out = temp / "writer.md"
            call("export", "本地学习链", 2, out)
            assert out.read_text() == SPEC and "专用标记" not in out.read_text()
            call("resume", append_job, config=dict(env, API_KEY=""))
            assert len(requests) == 9
            call("append", "本地学习链", 1, added, ok=False)
            assert len(requests) == 9
            checks.append("append_reads_parent_sources_relearns_without_old_style_and_saves_only_explicitly")

            # Configuration drift and a corrupted checkpoint never cause a fresh request.
            call("resume", append_job, config=dict(env, BASE_URL="http://127.0.0.1:1/v1"), ok=False)
            response = next((workspace / "jobs" / append_job / "requests").glob("*.response.json"))
            saved = response.read_bytes()
            broken = json.loads(saved)
            broken["response"]["content"] = "篡改响应"
            response.write_text(json.dumps(broken, ensure_ascii=False))
            call("resume", append_job, ok=False)
            response.write_bytes(saved)
            assert len(requests) == 9
            checks.append("endpoint_drift_and_corrupted_response_block_before_network")

            # Ctrl-C leaves a recoverable task and an uncertain request, without auto-retry.
            cancel_root = temp / "cancel-workspace"
            control["hold"] = True
            source.write_text(json.dumps({"articles": [a], "note": "中断验证"}, ensure_ascii=False))
            process = subprocess.Popen(argv("learn", source, root=cancel_root), cwd=temp, env=env,
                                       text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            assert received.wait(5)
            process.send_signal(signal.SIGINT)
            stdout, stderr = process.communicate(timeout=5)
            assert process.returncode == 1 and "学习已中断" in stderr
            cancelled_job = re.search(r"^job_id: ([0-9a-f]{64})$", stdout, re.M).group(1)
            release.set()
            control["hold"] = False
            count = len(requests)
            call("resume", cancelled_job, ok=False, root=cancel_root)
            assert len(requests) == count
            assert not (cancel_root / "versions").exists()
            checks.append("ctrl_c_retains_task_and_unknown_intent_without_changing_saved_versions")
            assert all("PRIVATE_ERROR_BODY" not in p.read_text() for p in workspace.rglob("*.json"))
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    after = hashlib.sha256(ledger.read_bytes()).hexdigest()
    assert before == after
    print(json.dumps({"status": "passed", "checks": checks, "local_http_requests": len(requests),
        "paid_model_requests": 0, "ledger_before_sha256": before, "ledger_after_sha256": after,
        "temporary_workspaces_cleaned": True}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
