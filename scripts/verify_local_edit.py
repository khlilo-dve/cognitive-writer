#!/usr/bin/env python3
"""Full-context / bounded-output editing, using real PTY and a local HTTP model."""
import http.server
import json
import os
from pathlib import Path
import sys
import tempfile
import threading

sys.dont_write_bytecode = True
from verify_repl_terminal import Terminal

ARTICLE = "# 标题\r\n\r\n重复段。\r\n\r\n重复段。\r\n\r\n- 列表一\r\n- 列表二\r\n\r\n> 引用保留\r\n\r\n```rs\r\nx();\r\n```\r\n\r\n尾段保留。\r\n"


def check_case(mode):
    requests = []
    edits = []
    arrived, release = threading.Event(), threading.Event()
    instruction = "第二段加一个案例"
    if mode in ("multi", "missing_id"):
        instruction = "第一段和第三段改得具体一些"
    elif mode in ("ambiguity", "restore", "cancel"):
        instruction = "把“重复段。”改得具体一些"

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(payload)
            user = payload["messages"][1]["content"]
            try:
                edit = json.loads(user)
            except ValueError:
                edit = None
            if edit is None:
                answer = json.dumps({"intent_type": "ModifySection", "instruction": instruction}, ensure_ascii=False)
            else:
                edits.append(edit)
                arrived.set()
                if mode == "interrupt":
                    release.wait(10)
                    return
                changes = [{"id": i, "replacement": f"修改块{i}。\n\n新增案例。"} for i in edit["allowed_ids"]]
                if mode == "outside_id":
                    changes.append({"id": 1, "replacement": "# 越界标题"})
                elif mode == "missing_id":
                    changes.pop()
                elif mode == "empty":
                    changes[0]["replacement"] = ""
                elif mode == "unclosed":
                    changes[0]["replacement"] = "```rs\n没有闭合"
                answer = "模型返回整篇文章" if mode == "invalid_json" else json.dumps({"edits": changes}, ensure_ascii=False)
            body = json.dumps({"choices": [{"message": {"content": answer}}]}, ensure_ascii=False).encode()
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
        with tempfile.TemporaryDirectory(prefix="cognitive-local-edit-") as temp:
            work = Path(temp)
            home = work / "home"
            session = home / ".cognitive-writer/current.json"
            session.parent.mkdir(parents=True)
            snapshot = {"state": "WaitingForFulltext", "current_fulltext": ARTICLE,
                        "current_slug": "original-slug", "current_style_content": "保留短句风格"}
            session.write_text(json.dumps(snapshot, ensure_ascii=False))
            env = {**os.environ, "HOME": str(home), "API_KEY": "local-only", "MODEL": "local-mock",
                   "BASE_URL": f"http://127.0.0.1:{server.server_port}", "WEBSITE_PATH": ""}
            terminal = Terminal(work, env)
            try:
                terminal.until("Enter 恢复，n 放弃")
                terminal.send("\n")
                terminal.until("[等待确认全文] > ")
                terminal.send(instruction + "\n")
                if mode in ("ambiguity", "restore", "cancel"):
                    terminal.until("[等待选择修改位置] > ")
                    saved = json.loads(session.read_text())
                    assert saved["pending_edit"]["candidate_ids"] == [2, 3]
                    assert saved["current_fulltext"] == ARTICLE
                    assert len(requests) == 1 and not edits
                    if mode == "cancel":
                        terminal.send("/cancel\n")
                        terminal.until("[等待确认全文] > ")
                        saved = json.loads(session.read_text())
                        assert saved["current_fulltext"] == ARTICLE and saved["pending_edit"] is None
                        terminal.send("/quit\n")
                        terminal.exited()
                        return {"case": mode, "status": "passed", "api_calls": len(requests)}
                    if mode == "restore":
                        terminal.send("/quit\n")
                        terminal.exited()
                        terminal.close()
                        terminal = Terminal(work, env)
                        terminal.until("Enter 恢复，n 放弃")
                        terminal.send("\n")
                        terminal.until("[等待选择修改位置] > ")
                    terminal.send("B99\n")
                    terminal.until("[等待选择修改位置] > ")
                    assert not edits
                    terminal.send("B3\n")
                if mode == "interrupt":
                    assert arrived.wait(5)
                    terminal.send(b"\x03")
                    terminal.exited()
                    saved = json.loads(session.read_text())
                    assert saved["current_fulltext"] == ARTICLE
                    assert saved["current_slug"] == "original-slug"
                    return {"case": mode, "status": "passed", "api_calls": len(requests)}
                failures = ("outside_id", "missing_id", "empty", "unclosed", "invalid_json")
                terminal.until("局部修改校验失败" if mode in failures else "指定内容修改完成")
                terminal.until("[等待确认全文] > ")
                assert len(edits) == 1 and len(requests) == 2
                payload = edits[0]
                assert payload["article"] == ARTICLE, "model did not receive full original article"
                assert payload["instruction"] == instruction
                assert payload["style"] == "保留短句风格"
                expected_ids = [2, 7] if mode in ("multi", "missing_id") else [3]
                assert payload["allowed_ids"] == expected_ids
                saved = json.loads(session.read_text())
                assert saved["current_slug"] == "original-slug", "body edit changed article identity"
                if mode in failures:
                    assert saved["current_fulltext"] == ARTICLE
                    assert saved["edit_undo"] is None
                else:
                    expected = ARTICLE
                    # Independently reconstruct with transmitted original byte offsets.
                    for block in reversed(payload["blocks"]):
                        if block["id"] in expected_ids:
                            data = expected.encode()
                            expected = (data[:block["start"]] + f"修改块{block['id']}。\n\n新增案例。".encode() + data[block["end"]:]).decode()
                    assert saved["current_fulltext"] == expected, "content outside selected ranges changed"
                    assert saved["edit_undo"]["fulltext"] == ARTICLE
                    if mode == "undo":
                        terminal.send("/quit\n")
                        terminal.exited()
                        terminal.close()
                        terminal = Terminal(work, env)
                        terminal.until("Enter 恢复，n 放弃")
                        terminal.send("\n")
                        terminal.until("[等待确认全文] > ")
                        terminal.send("/undo\n")
                        terminal.until("已恢复上一次局部修改前的全文")
                        terminal.until("[等待确认全文] > ")
                        assert json.loads(session.read_text())["current_fulltext"] == ARTICLE
                terminal.send("/quit\n")
                terminal.exited()
                assert not list(session.parent.glob("*.pending"))
                return {"case": mode, "status": "passed", "api_calls": len(requests)}
            finally:
                terminal.close()
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    for mode in ["single", "multi", "ambiguity", "restore", "cancel", "undo", "outside_id",
                 "missing_id", "empty", "unclosed", "invalid_json", "interrupt"]:
        print(json.dumps(check_case(mode), ensure_ascii=False), flush=True)
