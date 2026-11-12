#!/usr/bin/env python3
"""P7 fact-review PTY protocol checks, zero paid calls and isolated candidate workspaces."""
import hashlib,http.server,json,os,shutil,sys,tempfile,threading
from pathlib import Path
sys.dont_write_bytecode=True
from verify_learning_chat import ChatTerminal
from verify_repl_terminal import ROOT
OUT=ROOT/'research/style-aggregation/runs/quality-p7-local-01'
def main():
    ledger=ROOT/'research/style-aggregation/usage-ledger.json';before=hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests=[];errors=[];mode={'kind':'clear'}
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            try:
                b=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(b);s=b['messages'][0]['content'];u=b['messages'][1]['content'];kind=mode['kind']
                if '独立的文章事实与任务核对者' in s:
                    assert b['model']=='mimo-v2.6-pro' and b['response_format']=={'type':'json_object'}
                    if kind=='unknown':self.send_response(503);self.end_headers();return
                    draft=json.loads(u)['draft'];quote='不在原稿的句子' if kind=='badquote' else '排队刷卡'
                    answer={'issues':[{'kind':'unsupported_fact','quote':quote,'reason':'任务未提供排队或刷卡流程'}] if '排队刷卡' in draft or kind=='badquote' else [],'missing':[]}
                elif '针对独立事实核对' in s:
                    assert b['model']=='mimo-v2.6-flash';answer='修订后仍让所有人排队刷卡。' if kind=='repair-fail' else '图书馆十点闭馆。我希望留半小时收拾与回宿舍，所以把九点半作为自己停读的时间。这是我的安排。'
                elif b.get('response_format')=={'type':'json_object'}:
                    v=json.loads(u);answer={'analysis':'内部分析，事实与感受分开。','evidence':[{'article':a['article'],'quote':a['text']} for a in v['articles']]}
                elif b['model']=='mimo-v2.6-pro':answer='依托任务已有材料组织判断，保留已给感受与材料边界。'
                elif '结构设计专家' in s:answer='- 根据十点闭馆和半小时收尾安排结构'
                else:answer='图书馆十点闭馆，大家都在排队刷卡。我希望留半小时收拾与回宿舍。'
                text=answer if isinstance(answer,str) else json.dumps(answer,ensure_ascii=False)
                data=json.dumps({'model':b['model'],'choices':[{'message':{'content':text},'finish_reason':'stop'}],'usage':{'prompt_tokens':100,'completion_tokens':80}},ensure_ascii=False).encode()
                self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
            except Exception as e:errors.append(repr(e));self.send_response(500);self.end_headers()
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True;threading.Thread(target=server.serve_forever,daemon=True).start();checks=[]
    try:
      with tempfile.TemporaryDirectory(prefix='cw-p7-facts-') as tmp:
        work=Path(tmp);(work/'home').mkdir();(work/'styles').mkdir();shutil.copytree(ROOT/'prompts',work/'prompts');(work/'one.md').write_text('独特原文，灯下的书页依然安静。')
        env={**os.environ,'HOME':str(work/'home'),'API_KEY':'local-only','BASE_URL':f'http://127.0.0.1:{server.server_port}/v1','MODEL':'mimo-v2.6-flash','WEBSITE_PATH':''}
        def start(name,restore=False):
            r=work/name;t=ChatTerminal(work,env,r)
            if restore:t.until('Enter 恢复，n 放弃');t.send('\n');t.until('[风格学习任务] > ')
            else:t.until('[空闲] > ')
            return r,t
        def send(t,text,expected='[风格学习任务] > '):t.send(text+'\n');t.until(expected,timeout=15)
        def quit(t):t.send('/quit\n');t.exited();t.close()
        def state(r):return json.loads((r/'chat-session/current.json').read_text())
        def writer(r):
            paths=list((r/'chats').glob('*/writers/*/input.json'));assert len(paths)==1;return paths[0].parent
        brief='写一段夜读建议。已知图书馆十点闭馆，我希望留半小时收拾与回宿舍，不补未提供的设施或流程。'
        for kind in ['clear','repair-fail','badquote','unknown']:
            mode['kind']=kind;r,t=start(kind)
            try:
                send(t,'学习 "one.md"，保存为“夜读”');count=len(requests)
                send(t,'/use-version 夜读 1 '+brief,'[等待确认全文] > ' if kind=='clear' else '[风格学习任务] > ')
                root=writer(r);original=(root/'draft-original.md').read_text();assert '排队刷卡' in original
                if kind=='clear':
                    assert '排队刷卡' not in state(r)['current_fulltext'];audit=json.loads((root/'facts-audit.json').read_text());assert audit['repaired'] and audit['status']=='model_found_no_issue_after_one_repair'
                    assert (root/'draft-diff.md').exists() and len(requests)==count+5
                elif kind=='repair-fail':
                    assert state(r)['state']=='WaitingForLearningAgent';send(t,'/writer-review');n=len(requests);send(t,'继续');assert len(requests)==n
                    assert json.loads((root/'facts-audit.json').read_text())['status']=='needs_review_after_one_repair'
                    assert len(list((root/'requests').glob('writer-fact-repair-attempt-*.intent.json')))==1
                elif kind=='badquote':
                    assert not (root/'draft-revised.md').exists() and not (root/'facts-audit.json').exists();n=len(requests);send(t,'继续');assert len(requests)==n
                else:
                    n=len(requests);send(t,'继续');assert len(requests)==n;quit(t);r,t=start(kind,True);send(t,'继续');assert len(requests)==n
                    mode['kind']='clear';send(t,'/retry-learning','[等待确认全文] > ')
                    assert len(list((root/'requests').glob('writer-fact-review-attempt-*.intent.json')))==2
                    assert '排队刷卡' not in state(r)['current_fulltext']
                quit(t)
            finally:
                if not t.reaped:t.close()
            checks.append(kind)
      assert not errors,errors;assert hashlib.sha256(ledger.read_bytes()).hexdigest()==before
      report={'status':'complete','cases':checks,'local_model_requests':len(requests),'paid_requests':0,'ledger_unchanged':True,'errors':errors}
      OUT.mkdir(parents=True,exist_ok=True);(OUT/'facts-terminal.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps(report,ensure_ascii=False))
    finally:server.shutdown();server.server_close()
if __name__=='__main__':main()
