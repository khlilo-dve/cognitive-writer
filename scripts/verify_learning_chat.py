#!/usr/bin/env python3
"""P4 real PTY and local HTTP checks; no paid API or user files."""
import hashlib, http.server, json, os, pty, shutil, signal, sys, tempfile, threading
from pathlib import Path
sys.dont_write_bytecode=True
from verify_repl_terminal import Terminal, ROOT, BINARY

class ChatTerminal(Terminal):
    def __init__(self,work,env,root):
        self.pid,self.fd=pty.fork()
        if self.pid==0:
            os.chdir(work);os.execve(str(BINARY),[str(BINARY),'style-draft','--root',str(root),'chat'],env)
        self.output=b'';self.reaped=False

def main():
    ledger=ROOT/'research/style-aggregation/usage-ledger.json';before=hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests=[];downstream=[];errors=[];mode={'kind':'question'};arrived=threading.Event();release=threading.Event()
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            try:
                body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));(downstream if body['model']=='mimo-v2.6-pro' else requests).append(body)
                system=body['messages'][0]['content'];kind=mode['kind']
                if kind=='failure':self.send_response(503);self.end_headers();return
                if kind=='hold':arrived.set();release.wait(10)
                if '主会话输入解释器' in system:
                    assert body['model']=='mimo-v2.6-flash' and body['max_completion_tokens']==2048
                    assert body['thinking']=={'type':'disabled'} and body['response_format']=={'type':'json_object'}
                    user=json.loads(body['messages'][1]['content'])
                    if user['mode']=='reply':answer={'kind':'reply','scope':{'allow_partial':True} if kind=='invent-control' else {'article_ids':['s001-a002']}}
                    else:answer={'kind':'learn','sources':[{'kind':'file','path':'甲.md'}],'save_name':'研读'}
                elif '风格学习领域Agent' in system:
                    user=json.loads(body['messages'][1]['content']);n=user['clarifications_used']+1
                    answer={'tool':'clarify','topic':'选文','question':f'第{n}轮：请选择纸上候车或给团队的项目备忘？','reason':'两篇独立文章尚未选择'}
                elif '独立的文章事实与任务核对者' in system:
                    answer={'issues':[],'missing':[]}
                elif body.get('response_format')=={'type':'json_object'}:
                    v=json.loads(body['messages'][1]['content'])
                    answer={'analysis':'内部分析','evidence':[{'article':a['article'],'quote':a['text']} for a in v['articles']]}
                elif body['model']=='mimo-v2.6-pro':
                    answer='从具体材料组织声音，保留感受、推理和原文边界，不虚构亲历。'
                else:
                    answer='# 新文章\n材料按要求展开。' if '生成大纲' not in system else '## 主题\n展开主题。'
                text=answer if isinstance(answer,str) else json.dumps(answer,ensure_ascii=False)
                payload=json.dumps({'model':body['model'],'choices':[{'message':{'content':text},'finish_reason':'stop'}],'usage':{'prompt_tokens':100,'completion_tokens':60}},ensure_ascii=False).encode()
                self.send_response(200);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
            except (BrokenPipeError,ConnectionResetError):pass
            except Exception as e:errors.append(repr(e));self.send_response(500);self.end_headers()
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start();checks=[]
    try:
      with tempfile.TemporaryDirectory(prefix='cw-p4-pty-') as tmp:
        work=Path(tmp);home=work/'home';home.mkdir();(work/'styles').mkdir();(work/'styles/轻辩.md').write_text('# 写法\n专属规则BETA，按给定材料展开。')
        shutil.copytree(ROOT/'prompts',work/'prompts');(work/'甲.md').write_text('这是独立完整短文甲。');(work/'乙.md').write_text('另一个独立完整短文乙。')
        (work/'双文.md').write_text('# 纸上候车\n\n我把车票夹在书里。\n\n# 给团队的项目备忘\n\n上线前检查日志和数据回写。')
        env={**os.environ,'HOME':str(home),'API_KEY':'local-only','MODEL':'mimo-v2.6-flash','BASE_URL':f'http://127.0.0.1:{server.server_port}/v1','WEBSITE_PATH':''}
        def start(name,restore=False):
            root=work/name;t=ChatTerminal(work,env,root)
            if restore:t.until('Enter 恢复，n 放弃');t.send('\n');t.until('[风格学习任务] > ')
            else:t.until('[空闲] > ')
            return root,t
        def ref(root):return json.loads((root/'chat-session/current.json').read_text())['learning_chat']
        def parent(root):
            r=ref(root);return json.loads(sorted((root/'chats'/r['id']/'states').glob('*.json'))[-1].read_text())['state']
        def task(root):
            ident=parent(root)['active'];base=root/'agent-tasks'/ident
            return ident,json.loads(sorted((base/'states').glob('*.json'))[-1].read_text())['state'],json.loads((base/'plan.json').read_text())
        def quit(t):t.send('/quit\n');t.exited();t.close()
        def send(t,text,needle='[风格学习任务] > '):t.send(text+'\n');t.until(needle)

        count=len(requests);root,t=start('clear');original='学习这两篇 "甲.md" "乙.md"，保存为“笔记声音”'
        try:
            send(t,original);ident,s,p=task(root);assert s['status']=='ready_for_learning' and not s['questions'] and len(requests)==count
            assert p['delegate']['followup_request'] is None and p['delegate']['save_name']=='笔记声音' and p['delegate']['request']['expected_articles']==2
            quit(t);root,t=start('clear',True);send(t,'/learning');send(t,'继续');assert len(requests)==count
            send(t,'目标改为学习段落推进');assert task(root)[0]==ident and task(root)[1]['revision']==1
            send(t,'/cancel','[空闲] > ');assert not (root/'chat-session/current.json').exists();quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('clear_materials_save_name_zero_control_calls_restore_and_same_task_revision')

        root,t=start('choice');count=len(requests)
        try:
            send(t,'学习 "双文.md"，只学一篇');ident,s,p=task(root);assert len(s['questions'])==1 and len(requests)==count+1
            send(t,'/learning');send(t,'暂停学习');quit(t);root,t=start('choice',True)
            assert len(requests)==count+1;send(t,'继续');assert len(requests)==count+1
            send(t,'更想保留项目备忘那一篇');assert task(root)[1]['selected']==['s001-a002'] and len(requests)==count+2
            send(t,'/cancel','[空闲] > ');quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('question_binding_pause_restore_free_reply_and_no_duplicate_decision')

        root,t=start('missing');count=len(requests)
        try:
            send(t,'学习一种风格');ident,s,p=task(root);assert s['status']=='waiting_input' and len(s['questions'])==1
            send(t,'暂时没有');assert task(root)[1]['status']=='blocked' and len(requests)==count
            send(t,'正文：后来主动提供了完整短文。');assert task(root)[0]==ident and task(root)[1]['status']=='ready_for_learning' and len(task(root)[1]['questions'])==1
            quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('missing_source_one_domain_question_then_block_and_proactive_material_without_new_budget')

        root,t=start('use');count=len(requests);original='不要学习，用轻辩风格写一篇AI文章，800字，给学生看'
        try:
            send(t,original,'[等待确认全文] > ');assert len(requests)==count+2
            assert not (root/'agent-tasks').exists()
            saved=json.loads((root/'chat-session/current.json').read_text());assert saved['generation_request']==original
            assert all(original in json.dumps(b,ensure_ascii=False) and '专属规则BETA' in json.dumps(b,ensure_ascii=False) for b in requests[count:])
            quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('use_existing_style_stays_generation_and_preserves_original_requirements')

        root,t=start('replacement');count=len(requests)
        try:
            send(t,'学习 "不存在.md"');ident,s,p=task(root);assert s['status']=='waiting_input' and len(s['questions'])==1 and len(requests)==count
            send(t,'正文：提供替代来源的完整短文。');assert task(root)[0]==ident and task(root)[1]['status']=='ready_for_learning' and len(task(root)[1]['request']['sources'])==1 and len(requests)==count
            original='取消当前学习，用轻辩风格写一篇读书文章，600字'
            send(t,original,'[等待确认全文] > ');assert len(requests)==count+2
            old=json.loads(sorted((root/'agent-tasks'/ident/'states').glob('*.json'))[-1].read_text())['state'];assert old['status']=='cancelled'
            assert json.loads((root/'chat-session/current.json').read_text())['generation_request']==original
            quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('unreadable_source_one_question_direct_replacement_and_cancel_then_use_existing_style')

        mode['kind']='failure';root,t=start('failure');count=len(requests)
        try:
            send(t,'学习 "双文.md"');ident,s,p=task(root);assert s['status']=='failed' and len(requests)==count+1
            quit(t);root,t=start('failure',True);send(t,'继续');assert len(requests)==count+1
            mode['kind']='question';send(t,'/retry-learning');assert len(requests)==count+2 and task(root)[1]['status']=='waiting_input'
            bill=json.loads((root/'agent-tasks'/ident/'usage-ledger.json').read_text())['entries'];assert len(bill)==2 and sum(v['actual_micro'] is None for v in bill.values())==1
            quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('domain_failure_restore_no_implicit_resend_explicit_retry_keeps_unknown_fee')

        root,t=start('bounds');count=len(requests)
        try:
            send(t,'学习 "双文.md"，只学一篇');ident,s,p=task(root)
            send(t,'我还没决定');assert len(task(root)[1]['questions'])==2
            send(t,'目标改为学习段落推进');assert task(root)[1]['status']=='blocked' and len(task(root)[1]['questions'])==2
            send(t,'学习另一个风格');assert task(root)[0]==ident
            send(t,'/replace-learning 学习正文：新任务的独立短文。');assert task(root)[0]!=ident and task(root)[1]['status']=='ready_for_learning'
            quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('goal_revision_preserves_two_question_cap_and_only_explicit_replacement_creates_task')

        root,t=start('invent');count=len(requests)
        try:
            send(t,'学习 "双文.md"，只学一篇');ident,s,p=task(root);mode['kind']='invent-control'
            send(t,'我想稍微偏向叙述一点');assert len(requests)==count+2 and task(root)[1]['request']['allow_partial'] is False
            send(t,'继续');assert len(requests)==count+2
            send(t,'/cancel','[空闲] > ');quit(t)
        finally:
            if not t.reaped:t.close()
        mode['kind']='question';checks.append('unsupported_model_scope_change_rejected_and_cached_invalid_action_not_resampled')

        mode['kind']='hold';arrived.clear();release.clear();root,t=start('interrupt');count=len(requests)
        try:
            t.send('学习 "甲.md"，命名为研读\n');assert arrived.wait(10);t.send(b'\x03');t.exited();t.close();release.set()
            assert parent(root)['pending'] is not None and len(requests)==count+1
            root,t=start('interrupt',True);send(t,'继续');assert len(requests)==count+1
            mode['kind']='question';send(t,'/retry-learning');assert task(root)[1]['status']=='ready_for_learning' and len(requests)==count+2
            frozen=json.loads((root/'chats'/ref(root)['id']/'plan.json').read_text());assert frozen['prompt'] in requests[-1]['messages'][0]['content']
            quit(t)
        finally:
            release.set()
            if not t.reaped:t.close()
        checks.append('ctrl_c_during_parent_interpretation_keeps_pending_ref_and_explicit_retry')
    finally:
        release.set();server.shutdown();server.server_close();thread.join(timeout=5)
    assert not errors,errors
    after=hashlib.sha256(ledger.read_bytes()).hexdigest();assert before==after
    audit={'status':'passed','checks':checks,'local_http_requests':len(requests)+len(downstream),'parent_domain_and_legacy_writer_requests':len(requests),'learning_fact_requests':len(downstream),'paid_model_requests':0,'ledger_before_sha256':before,'ledger_after_sha256':after,'scope':'P4 conversation controls on current P5/P7 runtime; downstream Pro calls counted separately; combined learned Writer is covered by P5/P8 suites','temporary_workspaces_cleaned':True}
    path=ROOT/'research/style-aggregation/runs/learning-chat-local-01';path.mkdir(exist_ok=True)
    (path/'cli-audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(audit,ensure_ascii=False,indent=2))
if __name__=='__main__':main()
