#!/usr/bin/env python3
"""P8 actual terminal: mixed formats, versions, Writer review and recoverable pending work.
All model calls go to localhost; sources and state use a temporary workspace.
"""
import hashlib, http.server, json, os, shutil, sys, tempfile, threading
from pathlib import Path
sys.dont_write_bytecode = True
from verify_learning_chat import ChatTerminal
from verify_repl_terminal import ROOT
OUT = ROOT / 'research/style-aggregation/runs/product-acceptance-local-01'
FIXTURES = ROOT / 'research/style-aggregation/runs/learning-formats-local-01/fixtures'

def main():
    ledger = ROOT / 'research/style-aggregation/usage-ledger.json'
    before = hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests, errors, checks = [], [], []
    mode = {'kind': 'repair'}
    original = '图书馆十点闭馆。\n\n所有人都要排队刷卡。'
    revised = '图书馆十点闭馆。我希望留半小时收拾与回宿舍，所以建议九点半停读。'
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            try:
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                requests.append(body)
                system, user = [m['content'] for m in body['messages']]
                assert body['thinking'] == {'type': 'disabled'}
                if '主会话输入解释器' in system:
                    raw = json.loads(user)['input']
                    assert '追加到' in raw
                    answer = {'kind': 'learn', 'sources': [{'kind': 'text', 'label': '追加正文', 'text': raw.split('正文：', 1)[1]}], 'base_name': '研读', 'save_name': '研读'}
                elif '风格学习领域Agent' in system:
                    v = json.loads(user)
                    pending = [s for s in v['source_summary']['sources'] if s['status'] == 'needs_inspection' and not s.get('resolution')]
                    assert pending
                    source = pending[0]
                    if source['id'] not in v['fully_visible_sources'] and source['id'] not in v['inspected_sources']:
                        answer = {'tool': 'inspect', 'source_ids': [source['id']], 'reason': '检查完整PDF文字'}
                    else:
                        answer = {'tool': 'resolve', 'source_id': source['id'], 'classification': 'article', 'evidence': [source['candidates'][0]['body']['text'][:10]], 'reason': '连续完整文章，不按页分篇'}
                elif '独立的文章事实与任务核对者' in system:
                    assert body['model'] == 'mimo-v2.6-pro'
                    v = json.loads(user)
                    assert set(v) == {'request', 'draft'}
                    # No missing key: real provider compatibility observed in P7.
                    answer = {'issues': [{'kind': 'unsupported_fact', 'quote': '所有人都要排队刷卡。', 'reason': '任务没有提供设施或流程'}]} if v['draft'] == original else {'issues': []}
                    if mode['kind'] == 'false-clear' and len([b for b in requests if '独立的文章事实与任务核对者' in b['messages'][0]['content']]) % 2 == 0:
                        answer = {'issues': []}
                elif '针对独立事实核对' in system:
                    assert body['model'] == 'mimo-v2.6-flash'
                    assert 'SOURCE_SENTINEL' not in system + user and 'INTERNAL_' not in system + user
                    answer = original if mode['kind'] == 'false-clear' else revised
                elif body.get('response_format') == {'type': 'json_object'}:
                    v = json.loads(user)
                    answer = {'analysis': 'INTERNAL_ANALYSIS', 'evidence': [{'article': a['article'], 'quote': a['text']} for a in v['articles']]}
                elif body['model'] == 'mimo-v2.6-pro':
                    answer = 'INTERNAL_REVIEW' if '复读' in system and '修订' not in system else '从具体材料进入判断，保留感受、限定与句段变化，不编造来源。'
                else:
                    assert 'SOURCE_SENTINEL' not in system + user and 'INTERNAL_' not in system + user
                    assert '纸上候车' not in system + user and '雨天的图书馆' not in system + user
                    answer = '- 按十点闭馆及半小时收尾组织' if '结构设计专家' in system else original
                text = answer if isinstance(answer, str) else json.dumps(answer, ensure_ascii=False)
                payload = json.dumps({'model': body['model'], 'choices': [{'message': {'content': text}, 'finish_reason': 'stop'}], 'usage': {'prompt_tokens': 100, 'completion_tokens': 80}}, ensure_ascii=False).encode()
                self.send_response(200); self.send_header('Content-Length', str(len(payload))); self.end_headers(); self.wfile.write(payload)
            except Exception as e:
                errors.append(repr(e)); self.send_response(500); self.end_headers()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='cw-p8-') as tmp:
            work = Path(tmp)
            (work/'home').mkdir(); (work/'styles').mkdir()
            shutil.copytree(ROOT/'prompts', work/'prompts')
            for name in ['中文文章.docx', '中文文字.pdf']:
                shutil.copy2(FIXTURES/name, work/name)
            (work/'one.md').write_text('SOURCE_SENTINEL，灯光落在纸上，短句停住。')
            env = {**os.environ, 'HOME': str(work/'home'), 'API_KEY': 'local-only', 'MODEL': 'mimo-v2.6-flash', 'BASE_URL': f'http://127.0.0.1:{server.server_port}/v1', 'WEBSITE_PATH': ''}
            def start(name, restore=False, prompt='[风格学习任务] > '):
                root = work/name
                t = ChatTerminal(work, env, root)
                if restore:
                    t.until('Enter 恢复，n 放弃'); t.send('\n'); t.until(prompt)
                else: t.until('[空闲] > ')
                return root, t
            def send(t, text, prompt='[风格学习任务] > '):
                t.send(text+'\n'); t.until(prompt, timeout=15)
            def quit(t):
                t.send('/quit\n'); t.exited(); t.close()
            def saved(root): return json.loads((root/'chat-session/current.json').read_text())
            def parent(root):
                ref = saved(root)['learning_chat']
                return json.loads(sorted((root/'chats'/ref['id']/'states').glob('*.json'))[-1].read_text())['state']
            def versions(root): return sorted((root/'versions').rglob('v*.json'))
            def snapshot(root, name):
                target = OUT/name
                assert target.resolve().parent == OUT.resolve()
                if target.exists(): shutil.rmtree(target)
                shutil.copytree(root, target)
            brief = '写一段夜读建议。图书馆十点闭馆，我希望留半小时收拾与回宿舍，不补设施或流程。'
            root, t = start('mixed')
            try:
                send(t, '学习这2篇 "中文文章.docx" "中文文字.pdf"，保存为“研读”')
                assert versions(root), t.output.decode(errors='replace')
                v1 = json.loads(versions(root)[0].read_text())
                assert len(v1['candidate']['input']['articles']) == 2
                active = parent(root)['active']
                task = json.loads(sorted((root/'agent-tasks'/active/'states').glob('*.json'))[-1].read_text())
                assert not task['state']['questions']
                n = len(requests); send(t, '/preview'); send(t, '保存为研读'); assert len(requests) == n and len(versions(root)) == 1
                quit(t); root, t = start('mixed', True); send(t, '继续'); assert len(requests) == n
                send(t, '追加到研读，保存为研读；正文：SOURCE_SENTINEL，第三篇独立文字，我把窗关上。')
                v2 = json.loads(versions(root)[1].read_text())
                assert v2['previous'] == v1['id'] and len(v2['candidate']['input']['articles']) == 3
                send(t, '/versions 研读'); send(t, '/show 研读 1')
                send(t, '/use-version 研读 1 '+brief, '[等待确认全文] > ')
                assert saved(root)['current_style_name'] == '研读 v1' and saved(root)['current_fulltext'] == revised
                n = len(requests); send(t, '/writer-review', '[等待确认全文] > '); assert len(requests) == n and saved(root)['current_fulltext'] == revised
                n = len(requests); quit(t); root, t = start('mixed', True, '[等待确认全文] > ')
                assert saved(root)['current_fulltext'] == revised and len(requests) == n
                send(t, '/cancel', '[空闲] > ')
                send(t, '/use-version 研读 1 '+brief, '[等待确认全文] > ')
                assert len(requests) == n and len(versions(root)) == 2
                snapshot(root, 'accepted-workspace'); quit(t)
                checks.append('mixed_docx_pdf_zero_clarification_save_append_old_v1_fact_repair_and_restart_zero_resend')
            finally:
                if not t.reaped: t.close()
            mode['kind'] = 'false-clear'
            root, t = start('blocked')
            try:
                send(t, '学习 "one.md"，保存为“研读”')
                send(t, '/use-version 研读 1 '+brief)
                assert saved(root)['state'] == 'WaitingForLearningAgent' and len(versions(root)) == 1
                decisions = list(root.glob('chats/*/writers/*/facts-decision.json'))
                assert len(decisions) == 1
                decision = json.loads(decisions[0].read_text())
                assert decision['model_found_no_issue'] and decision['unchanged_issue_paragraphs']
                n = len(requests); send(t, '/writer-review'); send(t, '继续'); assert len(requests) == n
                quit(t); root, t = start('blocked', True); send(t, '继续'); assert len(requests) == n
                snapshot(root, 'blocked-workspace'); quit(t)
                checks.append('unchanged_flagged_paragraph_overrides_model_clear_and_restore_preserves_version_without_resend')
            finally:
                if not t.reaped: t.close()
        assert not errors, errors
        assert hashlib.sha256(ledger.read_bytes()).hexdigest() == before
        report = {'status': 'complete', 'checks': checks, 'local_model_requests': len(requests), 'paid_requests': 0, 'ledger_sha256': before, 'ledger_unchanged': True, 'temporary_workspaces_cleaned': True, 'scope': 'real PTY with localhost model responses; no real style fidelity or publishing claim'}
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT/'terminal-audit.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
        print(json.dumps(report, ensure_ascii=False))
    finally:
        server.shutdown(); server.server_close()

if __name__ == '__main__': main()
