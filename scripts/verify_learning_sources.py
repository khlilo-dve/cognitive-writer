#!/usr/bin/env python3
"""Exercise source collection CLI with local HTTP and temporary files; no model requests."""
import hashlib
import http.server
import json
import os
import re
import subprocess
import tempfile
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/cognitive-writer"


def main():
    ledger = ROOT / "research/style-aggregation/usage-ledger.json"
    before = hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            raise AssertionError("Collection must not call a model")

        def do_GET(self):
            requests.append(self.path)
            status, body = {
                "/article": (200, "<title>网页</title><nav>菜单</nav><article><header><h1>网页</h1></header><p>这是第一段。</p><p>这是第二段。</p><script>不进入正文</script></article>"),
                "/multi": (200, "<article><h1>甲</h1><p>正文甲。</p></article><article><h1>乙</h1><p>正文乙。</p></article>"),
                "/login": (200, "<form><input type=password></form>"),
                "/directory": (200, "<nav><a href='/article'>网页</a><a href='/multi'>多文页</a></nav>"),
                "/unavailable": (403, "blocked"),
            }.get(self.path, (404, "missing"))
            data = body.encode()
            self.send_response(status)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    checks = []
    try:
        with tempfile.TemporaryDirectory(prefix="cw-source-input-") as tmp:
            temp = Path(tmp)
            workspace = temp / "workspace"
            env = dict(os.environ, API_KEY="", BASE_URL=f"http://127.0.0.1:{server.server_port}/unexpected-model")
            source_dir = temp / "材料"
            source_dir.mkdir()
            (source_dir / "样文.md").write_text("# 本地文章\n\n这是本地材料，保留完整段落。")
            manifest = source_dir / "sources.json"

            def call(*args, ok=True):
                result = subprocess.run([str(BIN), "style-draft", "--root", str(workspace), *map(str, args)],
                    cwd=temp, env=env, capture_output=True, text=True, timeout=20)
                assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
                return result

            def collect(sources, **kwargs):
                manifest.write_text(json.dumps(dict(sources=sources, note="按提供材料学习写法", **kwargs), ensure_ascii=False))
                result = call("collect", manifest)
                ident = re.search(r"^collection_id: ([0-9a-f]{64})$", result.stdout, re.M).group(1)
                report = json.loads((workspace / "source-collections" / f"{ident}.json").read_text())
                return ident, report, result.stdout

            def url(path):
                return {"kind": "url", "url": f"http://127.0.0.1:{server.server_port}{path}"}

            ident, report, out = collect([{"kind":"file", "path":"样文.md"}, url("/article")], expected_articles=2)
            assert '"ready"' in out and len(report["records"]) == 2
            assert report["records"][1]["candidates"][0]["text"] == "网页\n这是第一段。\n这是第二段。"
            assert "菜单" in report["records"][1]["snapshots"][0]["body"]
            target = temp / "texts.json"
            call("export-texts", ident, target)
            exported = json.loads(target.read_text())
            assert set(exported) == {"articles", "note"} and len(exported["articles"]) == 2
            call("export-texts", ident, target, ok=False)
            (source_dir / "样文.md").unlink()
            count = len(requests)
            assert ident in call("collection", ident).stdout and len(requests) == count
            checks.append("file_manifest_relative_path_html_body_snapshot_export_and_keyless_restart_without_network")

            ident, report, out = collect([url("/multi")])
            assert '"needs_input"' in out and len(report["records"][0]["candidates"]) == 2
            call("export-texts", ident, temp / "multi.json", ok=False)
            checks.append("multiple_articles_not_silently_merged_or_exported")

            for path in ["/login", "/directory", "/unavailable"]:
                count = len(requests)
                ident, report, out = collect([url(path)])
                assert '"blocked"' in out and len(requests) == count + 1
                assert len(report["records"][0]["attempts"]) == 1
                call("export-texts", ident, temp / "blocked.json", ok=False)
                if path == "/directory":
                    assert report["records"][0]["links"][0]["url"].endswith("/article")
            checks.append("login_navigation_and_forbidden_sources_preserved_without_retry_or_fake_article")

            sources = [{"kind":"text", "label":"提供的正文", "text":"第一段。第二段。"}, url("/unavailable")]
            ident, report, out = collect(sources)
            assert '"needs_input"' in out
            ident, report, out = collect(sources, allow_partial=True)
            assert '"ready"' in out
            ident, report, out = collect(sources, allow_partial=True, expected_articles=2)
            assert '"needs_input"' in out
            checks.append("partial_permission_does_not_override_explicit_independent_article_count")

            ident, report, out = collect([{"kind":"text", "label":"甲", "text":"相同正文。"}, {"kind":"text", "label":"乙", "text":"相 同 正 文。"}])
            assert '"ready"' in out and report["records"][1]["candidates"][0]["duplicate_of"] == "s001-a001"
            path = workspace / "source-collections" / f"{ident}.json"
            report["records"][0]["snapshots"][0]["body"] = "篡改快照"
            path.write_text(json.dumps(report, ensure_ascii=False))
            call("collection", ident, ok=False)
            checks.append("normalized_deduplication_and_tampered_snapshot_rejection")
            assert not (workspace / "jobs").exists() and not (workspace / "versions").exists()
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    after = hashlib.sha256(ledger.read_bytes()).hexdigest()
    assert before == after
    audit = dict(status="passed", checks=checks, local_http_gets=len(requests), model_requests=0,
        ledger_before_sha256=before, ledger_after_sha256=after, temporary_workspace_cleaned=True,
        scope="P2 acquisition CLI; domain Agent, natural-language clarification and real web quality remain unverified")
    root = ROOT / "research/style-aggregation/runs/learning-input-local-01"
    root.mkdir(exist_ok=True)
    (root / "cli-audit.json").write_text(json.dumps(audit, ensure_ascii=False, indent=2) + "\n")
    print(f"Verified {len(checks)} source CLI scenarios, {len(requests)} local GETs, 0 model calls; ledger unchanged")


if __name__ == "__main__":
    main()
