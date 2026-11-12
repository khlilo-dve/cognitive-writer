#!/usr/bin/env python3
"""P5 real PTY + localhost model protocol; isolated files, no paid API."""
import hashlib, http.server, json, os, shutil, signal, sys, tempfile, threading
from pathlib import Path
sys.dont_write_bytecode = True
from verify_learning_chat import ChatTerminal
from verify_repl_terminal import ROOT

def main():
    ledger = ROOT/'research/style-aggregation/usage-ledger.json'
    before = hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests=[]; errors=[]; mode={'fail':False,'hold':False}; arrived=threading.Event(); release=threading.Event()
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args): pass
        def do_POST(self):
            try:
                b=json.loads(self.rfile.read(int(self.headers['Content-Length']))); requests.append(b)
                system=b['messages'][0]['content']; user=b['messages'][1]['content']
                assert b['thinking']=={'type':'disabled'}
                if mode['fail'] and b['model']=='mimo-v2.6-pro':
                    self.send_response(503);self.end_headers();return
                if mode['hold'] and b['model']=='mimo-v2.6-flash' and '主会话输入解释器' not in system:
                    arrived.set();release.wait(8)
                if '主会话输入解释器' in system:
                    raw=json.loads(user)['input']
                    assert '追加到' in raw
                    text=raw.split('正文：',1)[1]
                    answer={'kind':'learn','sources':[{'kind':'text','label':'正文','text':text}], 'base_name':'夜读','save_name':'夜读'}
                elif '独立的文章事实与任务核对者' in system:
                    assert b['model']=='mimo-v2.6-pro';answer={'issues':[],'missing':[]}
                elif b.get('response_format')=={'type':'json_object'}:
                    data=json.loads(user)
                    answer={'analysis':'INTERNAL_ANALYSIS','evidence':[{'article':a['article'],'quote':a['text']} for a in data['articles']]}
                elif b['model']=='mimo-v2.6-pro':
                    answer='INTERNAL_REVIEW' if '复读' in system and '修订' not in system else '从具体观察进入判断，保留推理的来路与适用条件，按表达需要安排节奏。'
                else:
                    assert 'SOURCE_SENTINEL' not in user+system and 'INTERNAL_' not in user+system
                    answer='# 新文章\n图书馆夜间阅读，只采用用户新材料。' if '正文' in system else '- 夜间阅读的安排'
                content=answer if isinstance(answer,str) else json.dumps(answer,ensure_ascii=False)
                payload=json.dumps({'model':b['model'],'choices':[{'message':{'content':content},'finish_reason':'stop'}], 'usage':{'prompt_tokens':150,'completion_tokens':60}},ensure_ascii=False).encode()
                self.send_response(200);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
            except (BrokenPipeError,ConnectionResetError): pass
            except Exception as e: errors.append(repr(e));self.send_response(500);self.end_headers()
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True
    threading.Thread(target=server.serve_forever,daemon=True).start();checks=[]
    try:
      with tempfile.TemporaryDirectory(prefix='cw-p5-') as tmp:
        work=Path(tmp);(work/'home').mkdir();(work/'styles').mkdir()
        shutil.copytree(ROOT/'prompts',work/'prompts');(work/'article.md').write_text('SOURCE_SENTINEL，完整独特文章，我把灯留在书页旁。')
        env={**os.environ,'HOME':str(work/'home'),'API_KEY':'local-only','MODEL':'mimo-v2.6-flash','BASE_URL':f'http://127.0.0.1:{server.server_port}/v1','WEBSITE_PATH':''}
        def start(name,restore=False):
            root=work/name;t=ChatTerminal(work,env,root)
            if restore:t.until('Enter 恢复，n 放弃');t.send('\n');t.until('[风格学习任务] > ')
            else:t.until('[空闲] > ')
            return root,t
        def send(t,text,prompt='[风格学习任务] > '):t.send(text+'\n');t.until(prompt,timeout=15)
        def quit(t):t.send('/quit\n');t.exited();t.close()
        def state(root):
            saved=json.loads((root/'chat-session/current.json').read_text());r=saved['learning_chat']
            return json.loads(sorted((root/'chats'/r['id']/'states').glob('*.json'))[-1].read_text())['state']
        def versions(root):return sorted((root/'versions').rglob('v*.json'))
        root,t=start('single')
        try:
            send(t,'学习 "article.md"');assert state(root)['delivery']['candidate_id'] and not versions(root)
            count=len(requests);send(t,'/preview');send(t,'/diff');send(t,'保存为夜读');send(t,'保存为夜读');assert len(versions(root))==1 and len(requests)==count
            quit(t);root,t=start('single',True);send(t,'继续');assert len(requests)==count
            send(t,'追加到夜读，保存为夜读；正文：另一个独立完整文章，我关掉走廊的灯。');assert len(versions(root))==2
            v1=json.loads(versions(root)[0].read_text());v2=json.loads(versions(root)[1].read_text());assert v2['previous']==v1['id'] and len(v2['candidate']['input']['articles'])==2
            send(t,'/show 夜读 1');send(t,'/versions 夜读');send(t,'/use-version 夜读 1 写图书馆夜读，800字，给学生看','[等待确认全文] > ')
            saved=json.loads((root/'chat-session/current.json').read_text());assert saved['current_style_name']=='夜读 v1' and '800字' in saved['generation_request']
            send(t,'/cancel','[空闲] > ')
            send(t,'用夜读风格写图书馆夜读，700字','[等待确认全文] > ')
            saved=json.loads((root/'chat-session/current.json').read_text());assert saved['current_style_name']=='夜读 v2'
            send(t,'/cancel','[空闲] > ')
            (work/'styles/夜读.md').write_text('旧同名写法，应明确选择。')
            count=len(requests);send(t,'用夜读风格写图书馆夜读');assert len(requests)==count
            assert state(root).get('selected_writer') is None
            send(t,'/use-version 夜读 1 写图书馆夜读','[等待确认全文] > ')
            (work/'styles/夜读.md').unlink()
            quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('file_candidate_preview_save_idempotence_restore_append_v2_and_old_v1_writer')
        root,t=start('combined')
        try:
            send(t,'学习 "article.md"，保存为“夜读”，再写图书馆夜读，600字','[等待确认全文] > ')
            saved=json.loads((root/'chat-session/current.json').read_text());assert saved['generation_request']=='写图书馆夜读，600字' and len(versions(root))==1
            assert len(list((root/'jobs').iterdir()))==1;quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('combined_auto_save_then_flash_writer_with_isolated_brief')
        root,t=start('paste')
        try:
            # Existing terminal continuation sends the entire article and trailing instruction as one message.
            t.send('# 随笔\\\nSOURCE_SENTINEL，文内提到 /tmp/private.md 和 https://example.com/story。\\\n\\\n学习这篇文章的风格\n');t.until('[风格学习任务] > ',timeout=15)
            d=state(root)['delivery'];assert d['candidate_id'] and not versions(root)
            job=json.loads((root/'jobs'/d['job_id']/'plan.json').read_text());assert len(job['articles'])==1 and '学习这篇文章的风格' not in job['articles'][0]
            assert len(requests)>0;send(t,'/use-candidate 写图书馆夜读，500字','[等待确认全文] > ');assert not versions(root);quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('natural_trailing_learning_instruction_and_unnamed_candidate_writer')
        root,t=start('unknown')
        try:
            mode['fail']=True;send(t,'学习 "article.md"');d=state(root)['delivery'];assert d['candidate_id'] is None
            count=len(requests);send(t,'继续');assert len(requests)==count
            quit(t);root,t=start('unknown',True);send(t,'继续');assert len(requests)==count
            mode['fail']=False;send(t,'/retry-learning');assert state(root)['delivery']['candidate_id']
            stats=list((root/'jobs'/d['job_id']/'requests').glob('learn-analysis-attempt-*.intent.json'));assert len(stats)==2
            assert not versions(root);quit(t)
        finally:
            mode['fail']=False
            if not t.reaped:t.close()
        checks.append('unknown_learning_no_implicit_resend_restore_then_explicit_retry')
        root,t=start('writer-interrupt')
        try:
            send(t,'学习 "article.md"，保存为“夜读”');count=len(requests)
            mode['hold']=True;t.send('/use-version 夜读 1 写图书馆夜读\n');assert arrived.wait(5);t.send(b'\x03');t.exited();t.close()
            mode['hold']=False;release.set();root,t=start('writer-interrupt',True)
            send(t,'继续');assert len(requests)==count+1
            send(t,'/retry-learning','[等待确认全文] > ');assert len(versions(root))==1;quit(t)
        finally:
            mode['hold']=False;release.set()
            if not t.reaped:t.close()
        checks.append('writer_interrupt_preserves_version_and_requires_explicit_retry')
        assert not errors,errors
      assert hashlib.sha256(ledger.read_bytes()).hexdigest()==before
      result={'status':'complete','checks':checks,'scenario_count':len(checks),'local_model_requests':len(requests),'paid_requests':0,'ledger_unchanged':True,'errors':errors}
      out=ROOT/'research/style-aggregation/runs/learning-flow-local-01';out.mkdir(parents=True,exist_ok=True)
      (out/'terminal-audit.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps(result,ensure_ascii=False))
    finally: release.set();server.shutdown();server.server_close()
if __name__=='__main__':main()
