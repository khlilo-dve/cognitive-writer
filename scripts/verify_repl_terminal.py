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

    def exited(self, timeout=3, expected_code=0):
        start = time.monotonic()
        while time.monotonic() - start < timeout:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.reaped = True
                assert os.waitstatus_to_exitcode(status) == expected_code, status
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
                snapshot["pending_style_content"] = "已提取的抽象规则" * (1000 if action == "write_failure" else 800 if action == "name" else 1)
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
                    assert (styles / "写一篇.md").read_text() == snapshot["pending_style_content"]
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


def check_restore_error(kind):
    with tempfile.TemporaryDirectory(prefix="cognitive-restore-") as temp:
        work = Path(temp)
        home = work / "home"
        session = home / ".cognitive-writer/current.json"
        session.parent.mkdir(parents=True)
        if kind == "unreadable":
            session.mkdir()
            (session / "keep").write_bytes(b"preserve")
        else:
            session.write_bytes(b'{broken' if kind == "corrupt" else b'\xff\xfe')
        original = session.read_bytes() if session.is_file() else None
        env = {**os.environ, "HOME":str(home), "API_KEY":"local-test-only", "WEBSITE_PATH":""}
        terminal = Terminal(work, env)
        try:
            terminal.until("原文件未改动")
            latency = terminal.exited(expected_code=1)
            if original is not None:
                assert session.read_bytes() == original
            else:
                assert (session / "keep").read_bytes() == b"preserve"
            return {"case":f"restore_{kind}_preserves_original", "status":"passed", "exit_seconds":latency}
        finally:
            terminal.close()


def check_draft_error(kind):
    with tempfile.TemporaryDirectory(prefix="cognitive-draft-") as temp:
        work = Path(temp)
        home = work / "home"
        session = home / ".cognitive-writer/current.json"
        session.parent.mkdir(parents=True)
        # Compact session fits under the file limit; rendered HTML exceeds it.
        body = "# 标题\n" + "**a** " * 80
        snapshot = {"state":"WaitingForFulltext", "current_fulltext":body, "current_slug":"draft"}
        session.write_text(json.dumps(snapshot, ensure_ascii=False))
        outputs = work / "outputs"
        website = work / "website"
        if kind == "local":
            outputs.write_text("blocked")
        elif kind == "website":
            website.mkdir()
            (website / "content").write_text("blocked")
        mock_bin = work / "bin"
        mock_bin.mkdir()
        for command, script in {"wslpath":"#!/bin/sh\nexit 1\n", "xclip":"#!/bin/sh\n/bin/cat >/dev/null\n"}.items():
            tool = mock_bin / command
            tool.write_text(script)
            tool.chmod(0o755)
        env = {**os.environ, "HOME":str(home), "API_KEY":"local-test-only", "MODEL":"local-mock",
               "PATH":str(mock_bin), "BASE_URL":"http://127.0.0.1:1",
               "WEBSITE_PATH":str(website) if kind == "website" else ""}
        terminal = Terminal(work, env, file_limit=1024 if kind == "html" else None)
        try:
            terminal.until("Enter 恢复，n 放弃")
            terminal.send("\n")
            terminal.until("[等待确认全文] > ")
            terminal.send("ok\n")
            terminal.until("草稿保存未完成")
            terminal.until("[等待确认全文] > ")
            saved = json.loads(session.read_text())
            assert saved["state"] == "WaitingForFulltext"
            assert saved["current_fulltext"] == body
            if kind == "local":
                assert outputs.read_text() == "blocked"
            else:
                assert (outputs / "draft_v1.md").read_text() == body
                assert not list(outputs.glob("*.pending"))
                if kind == "html":
                    assert not (outputs / "draft_v1.html").exists(), "partial HTML left behind"
                else:
                    assert (outputs / "draft_v1.html").is_file()
                    assert (website / "content").read_text() == "blocked"
            terminal.send("/quit\n")
            latency = terminal.exited()
            assert json.loads(session.read_text())["current_fulltext"] == body
            assert not list(session.parent.glob("*.pending"))
        finally:
            terminal.close()
        # Reopen the exact saved file and restore the article after the failure.
        terminal = Terminal(work, env)
        try:
            terminal.until("Enter 恢复，n 放弃")
            terminal.send("\n")
            terminal.until("[等待确认全文] > ")
            terminal.send("/quit\n")
            terminal.exited()
            assert json.loads(session.read_text())["current_fulltext"] == body
        finally:
            terminal.close()
        return {"case":f"draft_{kind}_failure_preserves_and_restores", "status":"passed", "exit_seconds":latency}


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
    for check, kinds in [(check_restore_error, ("corrupt", "invalid_utf8", "unreadable")),
                         (check_draft_error, ("local", "html", "website"))]:
        for kind in kinds:
            result = check(kind)
            results.append(result)
            print(json.dumps(result, ensure_ascii=False), flush=True)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps({"binary":str(BINARY), "checks":results}, ensure_ascii=False, indent=2)+"\n")

if __name__ == "__main__":
    main()
