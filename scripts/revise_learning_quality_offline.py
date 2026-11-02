#!/usr/bin/env python3
"""Derive explicitly labelled implementer edits; never calls a model or alters parent artifacts."""
import difflib
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "research/style-aggregation/runs/learning-jobs-live-02"
OUT = ROOT / "research/style-aggregation/runs/learning-quality-reviewed-01"


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        assert path.read_text() == text, f"Frozen artifact changed: {path}"
    else:
        path.write_text(text)


def main():
    ledger = ROOT / "research/style-aggregation/usage-ledger.json"
    before = digest(ledger.read_text())
    occupied = sum(e["actual_micro"] if e["actual_micro"] is not None else e["reserved_micro"]
                   for e in json.loads(ledger.read_text())["entries"].values()) / 1e6
    inputs = json.loads((BASE / "inputs.json").read_text())
    sources = {ident: text for pair in inputs["pairs"] for ident, text in zip(pair["source_ids"], pair["texts"])}
    edits = []
    rows = []
    for code in ["wxb", "wzq"]:
        for count in [1, 2]:
            ident = f"{code}-k{count}"
            link = json.loads((BASE / f"links/{ident}.json").read_text())
            directory = BASE / "workspaces" / code / "jobs" / link["job_id"]
            state = json.loads((directory / "state.json").read_text())
            candidate = json.loads((BASE / "workspaces" / code / "candidates" / f"{state['candidate_id']}.json").read_text())
            original = candidate["input"]["draft"]["spec"]
            spec = original

            def replace(old, new, reason, source, quote):
                nonlocal spec
                assert old in spec, (ident, old[:100])
                text = sources[source]
                assert quote in text, (source, quote)
                start = text.index(quote)
                spec = spec.replace(old, new, 1)
                edits.append({"id": ident, "before": old, "after": new, "reason": reason,
                              "evidence": {"source_id": source, "source_text_sha256": digest(text),
                                           "quote": quote, "start_codepoint": start, "end_codepoint": start + len(quote)}})

            def section(start, end, replacement, reason, source, quote):
                a = spec.index(start)
                b = spec.index(end, a) if end else len(spec)
                replace(spec[a:b], replacement + "\n\n", reason, source, quote)

            if ident == "wxb-k1":
                replace('说话人不是全知讲述者，而是一个有生活经验、懂点技术、带脾气、会冷嘲的旁观者。他的权威不来自专业知识，而来自阅历和常识判断。',
                        '说话人的可信感来自把见闻和判断分开说清：叙述中可以承认不懂，也可以用已有材料指出荒谬。本篇的技术经验是当篇论据，不要求新任务的说话人继承它。',
                        '当篇经验不升级为持续身份；保留叙述与判断的关系。', 'wxb-02', '虽然我也没学过医，但修好过一个闹钟')
                replace('说话人默认读者是**不了解所叙时代背景的当代人**。因此需要穿插简短的解释性旁白，把当年的常识、流行语、制度条件交代出来。',
                        '原文会在读者可能不熟悉的背景处插入短解释，让陌生材料变得可理解。新任务是否需要背景说明，取决于所给材料和读者需要，不预设读者处于某个时代。',
                        '迁移解释与读者的关系，不迁移原文时代坐标。', 'wxb-02', '现在的年轻人可能不知道')
                section('## 三、材料的推进逻辑', '## 四、四种表达',
                        '## 三、材料怎样增加判断的分量\n\n本篇让具体过程先有可追踪的展开，随后用新的细节改变读者对前文的理解。后来的判断并不只给事件贴标签，而是追问叙述里的解释究竟站不站得住。可迁移的是细节与解释之间逐步显露的落差。\n\n材料确有多层后果时，可以逐步加深分量；只有单一场景或一个概念问题时，就依照现有关系展开，不另造人物、层级或身体代价。判断也能在叙述中提前出现，与后续材料互相检验。',
                        '移除源文事件逐层重演，保留后续材料如何改变理解。', 'wxb-02', '我觉得有件事情最可恶')
                replace('**参照系多取自饮食、厨房、日常劳动一类**',
                        '**借熟悉而不相称的参照让读者感到落差，参照对象由新任务可用材料决定**',
                        '移除当篇比喻题材偏置。', 'wxb-02', '小时候我家附近有家小饭铺')
                replace('**最重的一次定性判断落在议论段末尾**，用词直接、情绪明确。',
                        '**明确的定性会使平静叙述突然显出态度**，可以与前后的叙述形成落差，不指定每篇最重判断的段落位置。',
                        '保留直接判断，去掉固定落点。', 'wxb-02', '我觉得有件事情最可恶')
                replace('四者的递进关系是：**叙事铺料 → 描写点睛 → 解释托底 → 判断收网**。"收网"指的是最重的那一刀落在议论段末尾，不是说前面不准有任何判断。',
                        '叙事、描写、解释和判断在本篇相互穿插：解释能暴露荒谬，短判断能改变细节的意味。使用时关注相接之处造成的语气变化，不把四种功能排成固定顺序。',
                        '功能关系不能再次变成四步模板。', 'wxb-02', '但我必须说他们一句好话')
                a=spec.index('- **结尾的咬合**');b=spec.index('## 八、',a)
                replace(spec[a:b], '- **判断之后怎样留下人的分量**：本篇在议论后回到人物经历，叙述因此没有被抽象定性完全取代。可以保留这种判断与人的处境互相牵制的关系；新材料也可以在明确判断处收住，不需要补齐自我调侃、回叙和温情等连续动作。\n\n',
                        '原文收尾路线只作局部依据，不写成新文章连续动作。', 'wxb-02', '住医院的事我还没写完呢')
                section('## 十二、成稿自检', None,
                        '## 十二、检查表达是否成立\n\n回看已有材料是否使判断获得分量，语气的松紧和转折是否帮助读者理解；所选关系只在它对本文有作用时保留。结尾能够回应本文的问题即可，不需要补齐本篇的动作顺序。事实、来源和身份仍按任务核对，不把声音需要变成虚构经历的理由。',
                        '自检与前文条件一致，不重新要求结尾路线或判断位置。', 'wxb-02', '把推论都写了出来，未免有直露之嫌')
            elif ident == "wzq-k1":
                replace('# 散文《昆明的雨》声音执行说明', '# 声音执行说明：具体材料与轻声判断',
                        '执行说明去除源文标题，来源身份留元数据。', 'wzq-02', '我想念昆明的雨。')
                replace('本篇的一个可辨识结构，是**首尾同一句短抒情判断句**独立成段收束全文：',
                        '本篇在前部铺垫之后给出一句短抒情判断，末尾再让它重现。可辨识的关系是：同一句话经过具体材料的展开后，带上前面未充分显露的分量。这不等于正文第一句就要直接抒情。',
                        '原文先由作画引入，不能将情感回声误报成实际第一句。', 'wzq-02', '宁坤要我给他画一张画')
                replace('**判断句的核心功能是中止论证**——它明确表达感受，但不展开"为什么是这种感受"。',
                        '**判断句有时让铺陈停顿、直接露出感受，也能引出解释或修正**。要看它与相邻语句的关系，不能把所有判断都当作论证的终点。',
                        '原文存在直接判断后继续解释的反例。', 'wzq-02', '因为是下下停停、停停下下')
                replace('- 情感判断可以明确、强烈，甚至用感叹号，但**判断之后不解释**——不转入冗长心理分析、不写情绪说教。',
                        '- 情感判断可以直接露出，也可以与具体说明相接；是否继续解释，由后文是否带来新的理解决定。本篇既有短促停顿，也有判断后展开细节的写法，不设“判断后不得解释”的禁令。',
                        '将无条件禁令还原为有变化的判断/解释关系。', 'wzq-02', '昆明的雨季是明亮的，丰满的，使人动情的。')
                replace('- 对话感可以通过**描写性声音再现**（如叫卖声、人声氛围的摹写）实现，不用对白推进情节，也不虚构对白。',
                        '- 任务确实提供的声音材料可以进入描写；没有声音记录时，以句法和语气形成对话感，不自行补造人声、叫卖或对白。',
                        '防止把描写性声音再现误读为可以凭空补声。', 'wzq-02', '卖杨梅的都是苗族女孩子')
                section('## 十一、成稿自检', None,
                        '## 十一、回看材料和感受的关系\n\n检查具体材料是否有各自的质地，短判断或较长解释是否让这些材料的意义更清楚。使用呼应时，后一次出现应因中间的展开而增加分量；没有采用呼应也无需补设。句段长短、插话和语体转换服务于本文，不按原文的首尾句、位置或次数检查。逐句核对事实与身份，材料缺失处不补造。',
                        '取消自检重设首尾同句、禁止论证等要求。', 'wzq-02', '我不记得昆明的雨季有多长')
            elif ident == "wxb-k2":
                replace('不要直接抒情。情绪通过反讽和荒诞感间接表达，具体落点见下。',
                        '这组文本中，情绪往往随着推演、反讽及明确判断显露；平静语调与尖锐态度可以并存。这里的材料不足以支持禁止直接情感表达，使用时看任务需要及前文如何承载态度。',
                        '未观察到某种抒情形式不等于可以普遍禁止它。', 'wxb-02', '我觉得有件事情最可恶')
                replace('这需要说话人在推进到漂亮结论后主动自我怀疑，让读者与作者共同怀疑。',
                        '当这个结论掩盖了材料里的矛盾时，可以让说话人追问它是否成立，带读者重新检查；没有这样的矛盾，就无需人为搭出再拆掉一个漂亮结论。',
                        '自我怀疑依真实张力发生，不强制每次搭建再拆解。', 'wxb-02', '但我怀疑这么理解是不对的')
                replace('纯粹抒情段落未在可见材料中出现，不要为了统一情感浓度而自行添加抒情段落，也不要为了统一理性风格而削平已有的长论证和明确情绪。',
                        '这组材料对其他抒情形态提供的依据有限，无法据此判定其一律不适用。保留当前材料实际需要的长论证与明确情绪，不为统一标签删减表达。',
                        '未知写成证据范围，不用未知继续设立风格禁令。', 'wxb-03', '对于一件愚蠢的事，你只能唱唱反调。')
            else:
                replace('文本说话人是刚完成一件具体事务、略具行业或常识的观察者，不是当事职业身份的代言人。',
                        '说话人从具体材料进入，愿意作判断，也保留对自己判断的限定。刚做完一件事只是其中一篇的写作缘由，不是采用这一声音所需的身份；另一篇的回望也不能变成每篇都要有的经历。',
                        '序言开头属于当篇缘由，不能泛化为持续身份。', 'wzq-04', '编完了这本书的稿子')
                replace('不要把 A 当作唯一核心，也不要在同一段落内混用两者的推进密度。',
                        '两种关系可以在同一段里转换或相接：具体材料能同时带来理解与感受。转换时看前后表达是否承接，不预先规定一段只能承担一种关系。',
                        '原文在同段穿插判断、材料与反问，未支持互斥分段。', 'wzq-04', '读了一篇文章，跟吃过一盘好菜毕竟不一样')
                replace('- 不要把两种机制混在同一段落：论证段走判断—检验，陈列段走概括—照亮，分段承担不同推进。',
                        '- 判断、具体说明和感受可以在同段相接；关注转接处是否使对象更清楚、情绪有来处，不按机制标签切分段落。',
                        '删除误用纠正中的互斥禁令。', 'wzq-04', '读了一篇文章，跟吃过一盘好菜毕竟不一样')
                section('## 九、成稿自检', None,
                        '## 九、回看所选写法\n\n检查判断是否与具体材料相互照应，细节是否带来新的理解或感受，解释、陈列和情绪之间的转换是否自然。所选组织只需对本文成立，不要求每个段落走完命名的路线，也不需要为了自谦、反复或收尾而增设动作。信息的未知、必写事实和身份边界照实保留。',
                        '末尾不再把条件路线与说话姿态变成逐项门槛。', 'wzq-02', '我不记得昆明的雨季有多长')

            assert spec != original
            relevant = [e for e in edits if e["id"] == ident]
            diff = ''.join(difflib.unified_diff(original.splitlines(keepends=True), spec.splitlines(keepends=True),
                                               fromfile='parent-model-spec', tofile='implementer-derived-spec', n=0))
            write(OUT / f"profiles/{ident}.md", spec)
            write(OUT / f"diffs/{ident}.diff", diff)
            row = {"id": ident, "status": "complete", "origin": "implementer_edit", "source_count": count,
                   "parent_candidate_id": candidate["id"], "parent_spec_sha256": digest(original),
                   "spec_sha256": digest(spec), "edits": relevant,
                   "status_quality": "draft_pending_semantic_review", "independent_evaluation": False}
            write(OUT / f"profiles/{ident}.json", json.dumps(row, ensure_ascii=False, indent=2) + '\n')
            rows.append({k: v for k, v in row.items() if k != 'edits'})
    assert before == digest(ledger.read_text())
    write(OUT / 'edits.json', json.dumps(edits, ensure_ascii=False, indent=2) + '\n')
    write(OUT / 'summary.json', json.dumps({"status": "complete", "stage": "implementer_grounded_revision",
        "rows": rows, "edit_count": len(edits), "model_requests": 0,
        "usage": {"run_occupied_cny": 0, "occupied_cny": occupied},
        "ledger_before_sha256": before, "ledger_after_sha256": before,
        "scope": "Derived examples for review, not output of the new automatic learner or a Writer validation."},
        ensure_ascii=False, indent=2) + '\n')
    print(f'Derived {len(rows)} labelled candidates, {len(edits)} grounded edits; zero calls, ledger unchanged')


if __name__ == '__main__':
    main()
