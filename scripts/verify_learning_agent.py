#!/usr/bin/env python3
"""P3 live-client protocol/CLI checks using a local server and temporary workspaces only."""
import hashlib
import http.server
import json
import os
import re
import shutil
import subprocess
import tempfile
import threading
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
BIN=ROOT/"target/debug/cognitive-writer"


def main():
    ledger=ROOT/"research/style-aggregation/usage-ledger.json"
    before=hashlib.sha256(ledger.read_bytes()).hexdigest()
    requests=[]
    control={"mode":"question"}
    received=threading.Event()
    release=threading.Event()
    errors=[]

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            try:
                body=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                requests.append(body)
                assert body["model"] in ("mimo-v2.6-flash","mimo-v2.6-pro")
                assert body["response_format"]=={"type":"json_object"}
                assert body["thinking"]=={"type":"disabled"} and body["max_completion_tokens"]==2048
                assert "CHANGED_PROMPT" not in body["messages"][0]["content"]
                user=json.loads(body["messages"][1]["content"])
                mode=control["mode"]
                if mode=="failure":
                    self.send_response(503);self.end_headers();self.wfile.write(b"PRIVATE_HTTP_ERROR");return
                if mode=="hold":
                    received.set();release.wait(10)
                if mode=="coherent":
                    decision={"tool":"inspect","source_ids":["s001"],"reason":"先检查正文"} if user["steps_used"]==0 else {"tool":"resolve","source_id":"s001","classification":"article","evidence":["同一份笔记还在整理"],"reason":"同一篇连续叙述的两个章节"}
                elif mode=="upgrade":
                    decision={"tool":"escalate","reason":"需要疑难判断复核"} if body["model"]=="mimo-v2.6-flash" else {"tool":"stop","reason":"当前范围仍无法可靠决定，任务保留"}
                else:
                    n=user["clarifications_used"]+1
                    decision={"tool":"clarify","topic":"选择文章","question":f"第{n}轮：请明确选择甲或乙的范围？","reason":"两篇独立文章尚未指定哪一篇，不能擅自选择"}
                model="unexpected-model" if mode=="wrong-model" else body["model"]
                content="```json\n{}\n```" if mode=="bad-json" else json.dumps(decision,ensure_ascii=False)
                payload={"model":model,"choices":[{"message":{"content":content},"finish_reason":"stop"}]}
                if mode!="missing-usage":payload["usage"]={"prompt_tokens":100,"completion_tokens":3000 if mode=="over-output" else 50}
                data=json.dumps(payload,ensure_ascii=False).encode()
                self.send_response(200);self.send_header("Content-Length",str(len(data)));self.end_headers();self.wfile.write(data)
            except (BrokenPipeError,ConnectionResetError):pass
            except Exception as e:
                errors.append(str(e));self.send_response(500);self.end_headers()

    server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Handler)
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    checks=[]
    try:
        with tempfile.TemporaryDirectory(prefix="cw-domain-agent-") as tmp:
            temp=Path(tmp);(temp/"prompts").mkdir();shutil.copyfile(ROOT/"prompts/learning_agent.md",temp/"prompts/learning_agent.md")
            env=dict(os.environ,API_KEY="fixture",BASE_URL=f"http://127.0.0.1:{server.server_port}/v1",MODEL="mimo-v2.6-flash")
            workspace=temp/"workspace"
            def argv(*args):return [str(BIN),"style-draft","--root",str(workspace),*map(str,args)]
            def call(*args,ok=True,config=env):
                r=subprocess.run(argv(*args),cwd=temp,env=config,capture_output=True,text=True,timeout=25)
                assert (r.returncode==0)==ok,(args,r.stdout,r.stderr)
                assert "PRIVATE_HTTP_ERROR" not in r.stdout+r.stderr
                return r
            def field(r,key):return re.search(rf"^{key}: ([0-9a-f]{{64}})$",r.stdout,re.M).group(1)
            def state(ident):
                files=sorted((workspace/"agent-tasks"/ident/"states").glob("*.json"))
                return json.loads(files[-1].read_text())["state"]
            def start(body="# 甲\n\n甲文章。\n\n# 乙\n\n乙文章。",budget=250000,ok=True):
                delegate={"user_request":"按提供的真实文章范围学习，不擅自选择","request":{"sources":[{"kind":"text","label":"材料","text":body}],"note":"学习写法"},"save_name":"用户已指定的名称","followup_request":"以后用此风格写新文章","budget_micro_cny":budget}
                path=temp/"delegate.json";path.write_text(json.dumps(delegate,ensure_ascii=False))
                r=call("agent-start",path,ok=ok);return field(r,"agent_task_id"),r
            def reply(ident,text,scope=None):
                s=state(ident);data={"expected_revision":s["revision"],"question_id":s["questions"][-1]["id"],"text":text,"scope":scope or {}}
                path=temp/"reply.json";path.write_text(json.dumps(data,ensure_ascii=False));return path

            count=len(requests);ident,r=start("这是完整的用户提供短文。")
            assert state(ident)["status"]=="ready_for_learning" and len(requests)==count
            call("agent-resume",ident,config=dict(env,API_KEY=""));assert len(requests)==count
            checks.append("normal_source_zero_models_zero_questions_and_keyless_ready_resume")

            control["mode"]="coherent";ident,r=start("# 早晨\n\n我先整理桌面的笔记。\n\n# 午后\n\n同一份笔记还在整理。")
            assert state(ident)["status"]=="ready_for_learning" and not state(ident)["questions"]
            checks.append("flash_inspect_and_resolve_sections_without_user_question")

            control["mode"]="question";ident,r=start();first_count=len(requests)
            assert state(ident)["status"]=="waiting_input"
            call("agent-resume",ident,config=dict(env,API_KEY=""));assert len(requests)==first_count
            call("agent-answer",ident,reply(ident,"选乙这一篇",{"article_ids":["s001-a002"],"expected_articles":1}))
            assert state(ident)["status"]=="ready_for_learning" and len(requests)==first_count
            checks.append("waiting_resume_no_request_and_explicit_user_selection_without_extra_question")

            ident,r=start();call("agent-answer",ident,reply(ident,"还没有确定，但偏向叙述"))
            assert len(state(ident)["questions"])==2
            call("agent-answer",ident,reply(ident,"仍然没法确认标题"))
            assert state(ident)["status"]=="blocked" and len(state(ident)["questions"])==2
            checks.append("at_most_two_questions_with_persisted_answers")

            control["mode"]="failure";ident,r=start(ok=False);count=len(requests)
            call("agent-resume",ident,ok=False);assert len(requests)==count
            # Pending user/system messages stay frozen after a prompt file change.
            (temp/"prompts/learning_agent.md").write_text("CHANGED_PROMPT_MUST_NOT_ENTER_PENDING_REQUEST")
            control["mode"]="question";call("agent-resume",ident,"--retry-uncertain")
            assert len(requests)==count+1
            bill=json.loads((workspace/"agent-tasks"/ident/"usage-ledger.json").read_text())["entries"]
            assert sum(e["actual_micro"] is None for e in bill.values())==1 and len(bill)==2
            shutil.copyfile(ROOT/"prompts/learning_agent.md",temp/"prompts/learning_agent.md")
            checks.append("unknown_request_not_resent_implicitly_explicit_retry_preserves_reserve_and_frozen_prompt")

            count=len(requests);ident,r=start(budget=1,ok=False)
            assert len(requests)==count and not (workspace/"agent-tasks"/ident/"requests").exists()
            checks.append("insufficient_budget_fails_before_request_intent")

            for mode in ["missing-usage","wrong-model","over-output"]:
                control["mode"]=mode;ident,r=start(ok=False);count=len(requests)
                call("agent-resume",ident,ok=False,config=dict(env,API_KEY=""));assert len(requests)==count
                assert state(ident)["status"]=="failed"
            checks.append("missing_usage_wrong_model_and_over_output_stop_with_receipt_and_no_new_sampling")

            control["mode"]="upgrade";count=len(requests);ident,r=start()
            assert [v["model"] for v in requests[count:]]==["mimo-v2.6-flash","mimo-v2.6-pro"]
            assert state(ident)["pro_used"] and state(ident)["status"]=="blocked"
            checks.append("single_pro_escalation_uses_same_durable_protocol")

            control["mode"]="hold";received.clear();release.clear()
            # Reuse a delegate containing ambiguous material; acquire its task ID before response.
            path=temp/"delegate.json";data=json.loads(path.read_text());data["budget_micro_cny"]=250000;path.write_text(json.dumps(data,ensure_ascii=False))
            process=subprocess.Popen(argv("agent-start",path),cwd=temp,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            assert received.wait(10)
            line=process.stdout.readline();ident=re.search(r"agent_task_id: ([0-9a-f]{64})",line).group(1)
            call("agent-cancel",ident);release.set();out,err=process.communicate(timeout=15)
            assert process.returncode==0,(out,err)
            assert state(ident)["status"]=="cancelled" and not state(ident)["questions"]
            assert list((workspace/"agent-tasks"/ident/"requests").glob("*.response.json"))
            checks.append("cancel_during_http_retains_late_receipt_without_adopting_question")
            assert not (workspace/"versions").exists()
    finally:
        release.set();server.shutdown();server.server_close();thread.join(timeout=5)
    assert not errors,errors
    after=hashlib.sha256(ledger.read_bytes()).hexdigest();assert before==after
    audit={"status":"passed","checks":checks,"local_model_requests":len(requests),"paid_model_requests":0,"ledger_before_sha256":before,"ledger_after_sha256":after,"temporary_workspaces_cleaned":True,"scope":"P3 domain-agent development CLI; P4/P5 product conversation and style generation not implemented"}
    root=ROOT/"research/style-aggregation/runs/learning-agent-local-01";root.mkdir(exist_ok=True)
    (root/"cli-audit.json").write_text(json.dumps(audit,ensure_ascii=False,indent=2)+"\n")
    print(f"Verified {len(checks)} domain-agent scenarios, {len(requests)} local model requests, 0 paid calls")


if __name__=="__main__":main()
