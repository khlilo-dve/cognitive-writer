#!/usr/bin/env python3
"""P6 independent document fixtures + real terminal + localhost sources/models, zero paid calls."""
import hashlib,http.server,json,os,re,shutil,sys,tempfile,threading,subprocess
from pathlib import Path
sys.dont_write_bytecode=True
from verify_learning_chat import ChatTerminal
from verify_repl_terminal import ROOT,BINARY
OUT=ROOT/'research/style-aggregation/runs/learning-formats-local-01'
FIXTURES=OUT/'fixtures'
def main():
    ledger=ROOT/'research/style-aggregation/usage-ledger.json';before=hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests=[];gets=[];errors=[];checks=[]
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_GET(self):
            gets.append(self.path);body='<article><h1>河边漫步</h1><p>散步回来，我把湿鞋放在门边。</p></article>'.encode()
            self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
        def do_POST(self):
            try:
                b=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(b)
                system=b['messages'][0]['content'];user=b['messages'][1]['content']
                assert b['thinking']=={'type':'disabled'}
                if '主会话输入解释器' in system:
                    v=json.loads(user);raw=v['input']
                    if v['mode']=='reply':
                        assert raw=='能读到的先用';answer={'kind':'reply','scope':{'allow_partial':True}}
                    else:
                        assert '追加到九篇' in raw
                        answer={'kind':'learn','sources':[{'kind':'file','path':p} for p in re.findall(r'"([^"\n]+\.(?:md|docx|pdf))"',raw)],'base_name':'九篇','save_name':'九篇'}
                elif '风格学习领域Agent' in system:
                    v=json.loads(user);sources=v['source_summary']['sources'];pool=v['source_summary']['current_pool']
                    unverified=[s for s in sources if s['status']=='needs_inspection' and not s.get('resolution')]
                    if unverified:
                        s=unverified[0]
                        if s['id'] not in v['fully_visible_sources'] and s['id'] not in v['inspected_sources']:
                            answer={'tool':'inspect','source_ids':[s['id']],'reason':'读取PDF完整文字候选'}
                        else:
                            quote=s['candidates'][0]['body']['text'][:10]
                            answer={'tool':'resolve','source_id':s['id'],'classification':'article','evidence':[quote],'reason':'完整文字层为连续文章，不按页分篇'}
                    else:
                        answer={'tool':'clarify','topic':'文章范围','question':'请选择要学习的文章范围，或明确只用能读到的来源。','reason':v.get('last_reason') or '数量/部分失败/输入预算需要明确范围'}
                elif b.get('response_format')=={'type':'json_object'}:
                    assert b['model']=='mimo-v2.6-pro';v=json.loads(user)
                    assert sum(len(a['text'].encode()) for a in v['articles'])<=256*1024
                    answer={'analysis':'仅根据本次文章观察写法。','evidence':[{'article':a['article'],'quote':a['text']} for a in v['articles']]}
                else:
                    assert b['model']=='mimo-v2.6-pro'
                    answer='依托已知材料组织感受与判断，随表达责任安排节奏，保留条件和未知，不补造场景。'
                text=answer if isinstance(answer,str) else json.dumps(answer,ensure_ascii=False)
                body=json.dumps({'model':b['model'],'choices':[{'message':{'content':text},'finish_reason':'stop'}],'usage':{'prompt_tokens':100,'completion_tokens':50}},ensure_ascii=False).encode()
                self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
            except Exception as e:errors.append(repr(e));self.send_response(500);self.end_headers()
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True;threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
      with tempfile.TemporaryDirectory(prefix='cw-p6-') as tmp:
        work=Path(tmp);(work/'home').mkdir();(work/'styles').mkdir();shutil.copytree(ROOT/'prompts',work/'prompts');shutil.copytree(FIXTURES,work/'fixtures')
        manifest=json.loads((OUT/'fixture-manifest.json').read_text())
        for i in range(1,12):(work/f'{i}.md').write_text(f'第{i}篇独立文章，内容代号{i}，保持完整正文。')
        (work/'extra-a.md').write_text('新增的独立文章甲。');(work/'extra-b.md').write_text('新增的独立文章乙。')
        env={**os.environ,'HOME':str(work/'home'),'API_KEY':'local-only','MODEL':'mimo-v2.6-flash','BASE_URL':f'http://127.0.0.1:{server.server_port}/v1','WEBSITE_PATH':''}
        def collect(name,names):
            root=work/name;root.mkdir();source=root/'sources.json';source.write_text(json.dumps({'sources':[{'kind':'file','path':str(work/'fixtures'/n)} for n in names],'note':'P6格式读取验证'},ensure_ascii=False))
            x=subprocess.run([str(BINARY),'style-draft','--root',str(root/'ws'),'collect',str(source)],cwd=work,env=env,capture_output=True,text=True,timeout=30);assert x.returncode==0,x.stderr
            paths=list((root/'ws/source-collections').glob('*.json'));assert len(paths)==1
            return root,json.loads(paths[0].read_text())
        root,c=collect('docx',['中文文章.docx','重复.md']);assert c['records'][0]['candidates'][0]['text']==manifest['expected_docx'];assert c['records'][1]['candidates'][0]['duplicate_of']=='s001-a001'
        for r in c['records']:
            if r.get('binary'):
                blob=root/'ws'/r['binary']['path'];assert hashlib.sha256(blob.read_bytes()).hexdigest()==r['binary']['sha256']
        # Recover from frozen binary even after the external source changes, then reject a damaged blob.
        (work/'fixtures/中文文章.docx').write_bytes(b'changed external file')
        x=subprocess.run([str(BINARY),'style-draft','--root',str(root/'ws'),'collection',c['id']],cwd=work,env=env,capture_output=True,text=True);assert x.returncode==0
        (root/'ws'/c['records'][0]['binary']['path']).write_bytes(b'corrupt snapshot')
        x=subprocess.run([str(BINARY),'style-draft','--root',str(root/'ws'),'collection',c['id']],cwd=work,env=env,capture_output=True,text=True);assert x.returncode!=0 and '快照损坏' in x.stderr
        shutil.copy(FIXTURES/'中文文章.docx',work/'fixtures/中文文章.docx');checks.append('word_visible_runs_table_hidden_text_dedup_and_frozen_binary_integrity')
        root,c=collect('pdf',['中文文字.pdf','空白加正文.pdf']);assert [a['text'] for a in c['records'][0]['candidates']]==['\n\n'.join(manifest['expected_pdf_pages'])];assert all(r['status']=='needs_inspection' for r in c['records']);assert '空白页[1]' in c['records'][1]['reason'];checks.append('chinese_ucs2_pdf_all_pages_one_article_and_blank_page')
        names=['扫描.pdf','混合扫描.pdf','加密.pdf','损坏.pdf','损坏.docx','含图文章.docx','解压超限.docx','原始.doc']
        root,c=collect('unsupported',names);assert all(not r['candidates'] for r in c['records']);assert '第2页' in c['records'][1]['reason'];assert all(r['status'] in ['unsupported','failed'] for r in c['records']);checks.append('scan_mixed_scan_encrypted_corrupt_embedded_and_zip_limits_never_return_partial_text')
        def start(name):
            root=work/name;t=ChatTerminal(work,env,root);t.until('[空闲] > ');return root,t
        def send(t,text,prompt='[风格学习任务] > '):t.send(text+'\n');t.until(prompt,timeout=20)
        def quit(t):t.send('/quit\n');t.exited();t.close()
        def parent(root):
            ref=json.loads((root/'chat-session/current.json').read_text())['learning_chat'];return json.loads(sorted((root/'chats'/ref['id']/'states').glob('*.json'))[-1].read_text())['state']
        def task(root):
            p=parent(root);return json.loads(sorted((root/'agent-tasks'/p['active']/'states').glob('*.json'))[-1].read_text())['state']
        root,t=start('mixed-chat');url=f'http://127.0.0.1:{server.server_port}/article'
        try:
            send(t,f'学习这3篇 "fixtures/中文文章.docx" "fixtures/中文文字.pdf" {url}，保存为“混合来源”')
            p=parent(root);assert p['delivery']['saved'];job=json.loads((root/'jobs'/p['delivery']['job_id']/'plan.json').read_text());assert len(job['articles'])==3 and not task(root)['questions'];assert len(gets)==1
            before_calls=len(requests);send(t,'继续');assert len(requests)==before_calls and len(gets)==1
            snapshot=OUT/'mixed-workspace';assert snapshot.resolve().parent==OUT.resolve()
            if snapshot.exists():shutil.rmtree(snapshot)
            shutil.copytree(root,snapshot);quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('docx_pdf_url_mixed_sources_to_saved_candidate_without_user_clarification_and_zero_call_resume')
        root,t=start('multi')
        try:
            send(t,'学习 "fixtures/两篇文章.docx"，只学一篇');assert task(root)['status']=='waiting_input' and len(task(root)['questions'])==1
            send(t,'选第二篇');p=parent(root);assert p.get('delivery'),json.dumps({'parent':p,'task':task(root),'output':t.output.decode(errors='replace')},ensure_ascii=False);job=json.loads((root/'jobs'/p['delivery']['job_id']/'plan.json').read_text());assert len(job['articles'])==1 and '项目备忘' in job['articles'][0] and '纸上候车' not in job['articles'][0];quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('multi_article_docx_one_bounded_question_then_exact_selected_article')
        root,t=start('partial')
        try:
            n=len([b for b in requests if b['model']=='mimo-v2.6-pro']);send(t,'学习 "fixtures/中文文章.docx" "missing.md"');assert task(root)['status']=='waiting_input' and len([b for b in requests if b['model']=='mimo-v2.6-pro'])==n
            send(t,'能读到的先用');assert parent(root).get('delivery'),json.dumps({'parent':parent(root),'task':task(root)},ensure_ascii=False);assert parent(root)['delivery']['candidate_id'];assert len(task(root)['questions'])==1;quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('missing_source_does_not_shrink_scope_until_explicit_partial_permission')
        root,t=start('eleven')
        try:
            files=' '.join(f'"{i}.md"' for i in range(1,12));n=len([b for b in requests if b['model']=='mimo-v2.6-pro']);send(t,'学习 '+files);assert task(root)['status']=='waiting_input' and len([b for b in requests if b['model']=='mimo-v2.6-pro'])==n
            send(t,'选择'+ '、'.join(f'第{i}篇' for i in range(1,11)));p=parent(root);job=json.loads((root/'jobs'/p['delivery']['job_id']/'plan.json').read_text());assert len(job['articles'])==10;quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('eleven_unique_articles_require_scope_then_ten_selected_without_truncation')
        root,t=start('append-limit')
        try:
            send(t,'学习这9篇 '+' '.join(f'"{i}.md"' for i in range(1,10))+'，保存为“九篇”');old=list((root/'versions').rglob('v000001.json'))[0].read_bytes()
            n=len([b for b in requests if b['model']=='mimo-v2.6-pro']);send(t,'追加到九篇，保存为九篇；学习 "extra-a.md" "extra-b.md"');assert task(root)['status']=='waiting_input';assert len([b for b in requests if b['model']=='mimo-v2.6-pro'])==n
            assert len(list((root/'versions').rglob('v*.json')))==1
            send(t,'选第一篇');p=parent(root);job=json.loads((root/'jobs'/p['delivery']['job_id']/'plan.json').read_text());assert len(job['articles'])==10
            assert list((root/'versions').rglob('v000001.json'))[0].read_bytes()==old and len(list((root/'versions').rglob('v*.json')))==2;quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('append_nine_plus_two_blocks_then_one_explicit_addition_fits_ten_without_dropping_old')
        root,t=start('long')
        try:
            n=len([b for b in requests if b['model']=='mimo-v2.6-pro']);send(t,'学习 "fixtures/长文章.docx"')
            assert not (root/'jobs').exists() and len([b for b in requests if b['model']=='mimo-v2.6-pro'])==n
            assert task(root)['status']=='waiting_input';quit(t)
        finally:
            if not t.reaped:t.close()
        checks.append('long_complete_article_retained_but_over_budget_never_sent_to_learning')
        # Direct learning CLI has the same byte budget gate, before job creation/API.
        p=work/'too-long.json';p.write_text(json.dumps({'articles':['x'*(256*1024+1)],'note':'超量输入'},ensure_ascii=False));n=len(requests)
        x=subprocess.run([str(BINARY),'style-draft','--root',str(work/'direct-limit'),'learn',str(p)],cwd=work,env=env,capture_output=True,text=True);assert x.returncode!=0 and '256KiB' in x.stderr and len(requests)==n
        checks.append('direct_learning_job_obeys_same_total_input_budget')
      assert not errors,errors;assert hashlib.sha256(ledger.read_bytes()).hexdigest()==before
      report={'status':'complete','checks':checks,'local_model_requests':len(requests),'source_http_requests':len(gets),'paid_requests':0,'ledger_before_sha256':before,'ledger_after_sha256':before,'errors':errors,'fixture_generator':'independent python-docx and ReportLab; product runtime Rust only'}
      (OUT/'terminal-audit.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps(report,ensure_ascii=False))
    finally:server.shutdown();server.server_close()
if __name__=='__main__':main()
