#!/usr/bin/env python3
"""Real Linux PTY regression; all user files live in TemporaryDirectory, no live API."""
import argparse
import http.server
import json
import os
from pathlib import Path
import pty
import resource
import select
import shutil
import signal
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/cognitive-writer"

class Terminal:
    def __init__(self, work, env, file_limit=None):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            if file_limit is not None:
                signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
                resource.setrlimit(resource.RLIMIT_FSIZE, (file_limit, file_limit))
            os.chdir(work)
            os.execve(str(BINARY), [str(BINARY)], env)
        self.output = b""
        self.reaped = False

    def send(self, value):
        os.write(self.fd, value.encode() if isinstance(value, str) else value)

    def until(self, text, timeout=5):
        expected = text.encode()
        deadline = time.monotonic() + timeout
        while expected not in self.output:
            if time.monotonic() > deadline:
                raise AssertionError(f"timeout waiting for {text!r}: {self.output.decode(errors='replace')}")
            if select.select([self.fd], [], [], 0.1)[0]:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    chunk = b""
                if not chunk:
                    raise AssertionError(f"exited before {text!r}: {self.output.decode(errors='replace')}")
                self.output += chunk
        self.output = self.output[self.output.index(expected) + len(expected):]

    def exited(self, timeout=3):
        start = time.monotonic()
        while time.monotonic() - start < timeout:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.reaped = True
                assert os.waitstatus_to_exitcode(status) == 0, status
                return round(time.monotonic() - start, 3)
            time.sleep(0.02)
        raise AssertionError("REPL did not exit promptly; possible blocked stdin/runtime")

    def close(self):
        if not self.reaped:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        os.close(self.fd)


def check_case(name, state=None, action="interrupt", model=False, restore_only=False):
    requests = []
    arrived = threading.Event()
    release = threading.Event()
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            arrived.set()
            if action == "interrupt":
                release.wait(10)
            else:
                body = json.dumps({"choices":[{"message":{"content":"# 新正文\n已从旧大纲恢复"}}]}, ensure_ascii=False).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="cognitive-pty-") as temp:
            work = Path(temp)
            home = work / "home"
            session = home / ".cognitive-writer/current.json"
            session.parent.mkdir(parents=True)
            styles = work / "styles"
            styles.mkdir()
            (styles / "保留.md").write_text("保留内容")
            shutil.copytree(ROOT / "prompts", work / "prompts")
            snapshot = {"state":state}
            if state == "WaitingForStyleName":
                snapshot["pending_style_content"] = "已提取的抽象规则" * (1000 if action == "write_failure" else 1)
            if state == "WaitingForDeleteStyleConfirmation":
                snapshot["pending_delete_name"] = "保留"
            if state == "WaitingForOutline":
                snapshot.update(current_outline="旧大纲", current_topic="旧主题", current_style_content="短句")
            if state:
                session.write_text(json.dumps(snapshot, ensure_ascii=False))
            env = {**os.environ, "HOME":str(home), "API_KEY":"local-test-only", "MODEL":"local-mock",
                   "BASE_URL":f"http://127.0.0.1:{server.server_port}", "WEBSITE_PATH":""}
            original_session = session.read_bytes() if session.exists() else None
            terminal = Terminal(work, env, file_limit=1024 if action == "write_failure" else None)
            try:
                if state:
                    terminal.until("Enter 恢复，n 放弃")
                    if not restore_only:
                        terminal.send("\n")
                if not restore_only:
                    terminal.until("] > ")
                if model:
                    terminal.send("ok\n" if state else "请帮我写文章\n")
                    assert arrived.wait(5), "mock API was not called"
                if action == "write_failure":
                    terminal.send("失败风格\n")
                    terminal.until("会话保存失败")
                    terminal.until("[等待风格命名] > ")
                    assert not (styles / "失败风格.md").exists(), "partial style left behind"
                    assert session.read_bytes() == original_session, "failed save truncated previous session"
                    assert not list(session.parent.glob("*.pending"))
                    terminal.send("/quit\n")
                    terminal.until("请保存内容或修复目录后重试退出")
                    terminal.until("[等待风格命名] > ")
                    terminal.send("/cancel\n/quit\n")
                elif action == "interrupt":
                    terminal.send(b"\x03")
                elif action == "eof":
                    terminal.send(b"\x04")
                elif action == "name":
                    terminal.send("写一篇\n")
                    terminal.until("风格已保存")
                    terminal.until("[空闲] > ")
                    terminal.send("/quit\n")
                elif action == "render":
                    terminal.until("[等待确认全文] > ")
                    terminal.send("/quit\n")
                latency = terminal.exited()
                assert (styles / "保留.md").read_text() == "保留内容"
                if action == "write_failure":
                    assert not session.exists()
                elif action == "name":
                    assert (styles / "写一篇.md").read_text() == "已提取的抽象规则"
                    assert not session.exists()
                elif state:
                    saved = json.loads(session.read_text())
                    expected = "WaitingForFulltext" if action == "render" else state
                    assert saved["state"] == expected, saved
                    for key in ("pending_style_content", "pending_delete_name", "current_outline"):
                        if key in snapshot:
                            assert saved[key] == snapshot[key]
                if not model:
                    assert not requests, "local input unexpectedly called classification"
                else:
                    assert len(requests) == 1
                    if state == "WaitingForOutline":
                        assert "旧大纲" in json.dumps(requests, ensure_ascii=False)
                return {"case":name, "status":"passed", "exit_seconds":latency, "api_calls":len(requests)}
            finally:
                terminal.close()
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    cases = [
        ("failed_write_is_atomic", {"state":"WaitingForStyleName", "action":"write_failure"}),
        ("idle_ctrl_c", {}),
        ("naming_ctrl_c", {"state":"WaitingForStyleName"}),
        ("delete_ctrl_c", {"state":"WaitingForDeleteStyleConfirmation"}),
        ("restore_prompt_ctrl_c", {"state":"WaitingForStyleName", "restore_only":True}),
        ("naming_eof", {"state":"WaitingForStyleName", "action":"eof"}),
        ("delete_eof", {"state":"WaitingForDeleteStyleConfirmation", "action":"eof"}),
        ("restored_naming_is_local", {"state":"WaitingForStyleName", "action":"name"}),
        ("classification_ctrl_c", {"model":True}),
        ("rendering_ctrl_c", {"state":"WaitingForOutline", "model":True}),
        ("old_outline_render", {"state":"WaitingForOutline", "model":True, "action":"render"}),
    ]
    results = []
    for name, kwargs in cases:
        result = check_case(name, **kwargs)
        results.append(result)
        print(json.dumps(result, ensure_ascii=False), flush=True)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps({"binary":str(BINARY), "checks":results}, ensure_ascii=False, indent=2)+"\n")

if __name__ == "__main__":
    main()
