#!/usr/bin/env python3
"""Style clarification end-to-end regression with local HTTP and isolated HOME."""
import argparse
import http.server
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import threading

sys.dont_write_bytecode = True
from verify_repl_terminal import ROOT, Terminal


def check_case(mode):
    requests = []
    chosen_query = {
        "direct": "轻辩", "unknown": "不存在", "ambiguous": "qingbian",
        "descriptive": "犀利", "invented": "轻辩",
    }.get(mode, "")
    request = "写一篇 AI 焦虑，800 字，给学生看"
    if mode in ("direct", "unknown", "ambiguous", "descriptive"):
        request += "，用" + chosen_query + "风格"

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(payload)
            if len(requests) == 1:
                answer = json.dumps({"intent_type": "Generate", "topic": "AI 焦虑",
                                     "style_name": chosen_query}, ensure_ascii=False)
            else:
                answer = "# 结果\n生成的正文" if len(requests) == 3 else "## 大纲\n讨论 AI 焦虑"
            body = json.dumps({"choices": [{"message": {"content": answer}}]}, ensure_ascii=False).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="cognitive-style-choice-") as temp:
            work = Path(temp)
            home = work / "home"
            home.mkdir()
            styles = work / "styles"
            styles.mkdir()
            # File names intentionally use pinyin; user replies use Chinese.
            (styles / "qingbian.md").write_text("# 规则\n犀利，短句。\n专属规则 BETA", encoding="utf-8")
            if mode != "single":
                (styles / "zeta.md").write_text("# 规则\n犀利，长句。\n专属规则 ZETA", encoding="utf-8")
            if mode == "ambiguous":
                (styles / "qingbian.md").rename(styles / "轻辩.md")
                (styles / "清变.md").write_text("# 规则\n温和叙述。", encoding="utf-8")
            shutil.copytree(ROOT / "prompts", work / "prompts")
            env = {**os.environ, "HOME": str(home), "API_KEY": "local-only", "MODEL": "local-mock",
                   "BASE_URL": f"http://127.0.0.1:{server.server_port}", "WEBSITE_PATH": ""}
            session = home / ".cognitive-writer/current.json"
            terminal = Terminal(work, env)
            try:
                terminal.until("[空闲] > ")
                terminal.send(request + "\n")
                if mode != "direct":
                    terminal.until("[等待选择风格] > ")
                    assert len(requests) == 1, "generation started before selection"
                    saved = json.loads(session.read_text())
                    assert saved["generation_request"] == request
                    assert saved["current_topic"] == "AI 焦虑"
                    assert saved["state"] == "WaitingForGenerationStyle"
                    if mode == "single":
                        assert len(saved["pending_style_choices"]) == 1
                    if mode in ("cancel", "interrupt", "eof", "restore"):
                        if mode == "cancel":
                            terminal.send("/cancel\n")
                            terminal.until("[空闲] > ")
                            assert not session.exists()
                            terminal.send("/quit\n")
                        else:
                            terminal.send({"interrupt": b"\x03", "eof": b"\x04", "restore": "/quit\n"}[mode])
                        terminal.exited()
                        assert len(requests) == 1
                        if mode != "cancel":
                            saved = json.loads(session.read_text())
                            assert saved["generation_request"] == request
                            assert saved["state"] == "WaitingForGenerationStyle"
                        if mode != "restore":
                            return {"case": mode, "status": "passed", "api_calls": len(requests)}
                        terminal.close()
                        # New earlier-sorting file must not shift the saved candidate numbering.
                        (styles / "aaa.md").write_text("新风格", encoding="utf-8")
                        terminal = Terminal(work, env)
                        terminal.until("Enter 恢复，n 放弃")
                        terminal.send("\n")
                        terminal.until("1. qingbian")
                        terminal.until("[等待选择风格] > ")
                        terminal.send("1\n")
                    else:
                        terminal.send("有哪些风格\n")
                        terminal.until("[等待选择风格] > ")
                        terminal.send("好\n")
                        terminal.until("还需要选择具体风格")
                        terminal.until("[等待选择风格] > ")
                        assert len(requests) == 1
                        terminal.send("用轻辩风格\n")
                terminal.until("[等待确认全文] > ")
                assert len(requests) == 3, "selection should go directly to outline and body"
                for payload in requests[1:]:
                    content = "\n".join(m["content"] for m in payload["messages"])
                    assert request in content, "original length/audience requirements were lost"
                    assert "专属规则 BETA" in content, "wrong style was selected"
                terminal.send("/quit\n")
                terminal.exited()
                saved = json.loads(session.read_text())
                assert saved["state"] == "WaitingForFulltext"
                assert saved["generation_request"] == request
                assert saved["current_style_name"] == ("轻辩" if mode == "ambiguous" else "qingbian")
                assert not saved["pending_style_choices"]
                return {"case": mode, "status": "passed", "api_calls": len(requests)}
            finally:
                terminal.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    results = []
    for mode in ["missing", "single", "unknown", "ambiguous", "descriptive", "invented",
                 "direct", "cancel", "interrupt", "eof", "restore"]:
        result = check_case(mode)
        results.append(result)
        print(json.dumps(result, ensure_ascii=False), flush=True)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps({"checks": results}, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
