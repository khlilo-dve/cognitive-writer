# 风格学习子Agent技术设计

2026-11-11 · v0.5 · 需求依据：[PRD](../PRD.md) SL-01至SL-16；[行动计划](../plans/2026-11-03-style-learning-agent-plan.md) P1–P8。本设计先于新增实现落盘。用户已指示“按照计划开始开发”，原P0仅文档限制已由该指令结束；不需要重复确认已批准方向。

## 1. 复用边界与架构决定

保留Rust主程序和REPL唯一stdin读取器。主Agent负责顶层意图与对话，子Agent是同进程内持有独立任务状态、模型决策及受限工具集合的领域角色。不会把每个节点做成独立Agent，也不先引入通用Graph Runtime。

复用learning_job的完整正文学习／原文复读、LearningWorkspace的候选与版本、learning_transport的请求身份与未知费用边界。P2只做输入工具，不能称子Agent已经实现。旧learn默认路径不改。

网页解析新增scraper 0.29（以Cargo.lock实际解析版本为准），用于HTML DOM与CSS选择；不复用旧strip_html_tags作为新输入链的文章判断。官方API依据：[scraper](https://docs.rs/scraper/0.29.0/scraper/)。网络沿用reqwest 0.12。

## 2. 主子Agent交接合同（P3/P4实施）

本节为主子Agent产品级概念合同；P3具体序列化字段与命令以第8/9节及learning_agent_types.rs为准，P4尚未接入。

Delegate携带task_id、revision、original_request、source_request引用、base_version、known_answers、save_intent、followup_writing_request和budget。TaskEvent由工具／子Agent产生，包含相同task_id/revision、工具身份和结果引用。Parent收到NeedsInput、CandidateReady、Saved、Blocked、Failed或Cancelled事件，不靠自然语言猜状态。

主Agent转呈子Agent的一个集中问题，用户回答以同一任务与问题ID回传；不新增stdin读取。一个会话最多一个活跃学习任务。修改目标增加revision，旧回执留在原修订，不可改写新修订候选；取消同样保留原请求和实际用量。

短期任务保存于现有学习工作区的agent-tasks目录，持久化task、revision、source_collection_id、selected_article_ids、known_answers、pending_question、clarification_rounds、decision_steps、no_progress_count、job_id和candidate/version引用。长期命名版本仍只使用LearningWorkspace，不另建版本库。

## 3. 状态与决策（P3/P4实施）

状态为active、waiting_input、candidate_ready、saved、blocked、failed、cancelled。active可选择collect_sources、inspect_sources、select_articles、learn_texts、preview_candidate、save_version或finish；澄清通过NeedsInput事件，不是工具内部读用户输入。

工具返回结构化结果，Agent决定下一步；数量、来源完整性、引用及保存身份由程序校验，模型不能覆盖。已清楚的路由和固定计数不调用模型；简单决策用Flash，有实质目标疑难时最多一次Pro升级。学习分析与综合仍固定Pro。模型适配在P3实施，P2不调用模型。

默认1轮集中澄清、最多2轮；第二轮须有用户新增必要信息。12次Agent决策不含学习工具内部阶段、用户等待和批量工具逐源读取。连续两次无状态／材料／结果进展收束。等待不占用模型；不靠新任务ID清零计数。

## 4. P2输入工具合同（本轮实施）

### 4.1 来源请求

SourceRequest包含sources、note、expected_articles（可选独立篇数）、allow_partial（默认false）。SourceInput为tagged enum：file/path，text/label/text，url/url。文件相对路径按来源请求JSON所在目录解析，直接API调用需显式base_dir；报告保留原定位与实际解析定位。未来DOCX和文字PDF在同一file入口扩展，P2只支持UTF-8 TXT/MD与UTF-8正文。

来源数与文章数分开；同一来源可产生多个ArticleCandidate。候选携带固定ID、source_id、title、text、text_sha256、normalized_sha256及boundary_method。去重只按忽略空白后的正文哈希，副本保留duplicate_of及原来源；不能把重复计多篇。

### 4.2 获取与识别

每来源最多两次不同路径：先直接HTTP获取；网络／服务端故障或正文边界仍未知时允许一次Jina Reader备用。401/403、明确登录／非文章页、已识别多文或摘要卡片不反复抓取。已有文本候选先领域检查，不为语义确认重复调用Reader。客户端超时20秒、最多5次重定向；测试通过注入备用前缀使用本地HTTP，不访问真实Reader。

结构化HTML先看独立article／articleBody／微信公众号正文容器，再看main／明确标题与正文段落。提取排除script/style/nav/站点header/footer/aside/form等非正文，排除隐藏节点，保留article内部标题头部、段落与行内连接；没有明确正文边界时返回needs_inspection，而不是接受整页去标签结果。article嵌套只保留外层，多个同级article产生多个候选，不默认全选。

Markdown／纯文本按提供的完整正文处理；多个一级标题形成多个候选，这只是需确认的边界信号，不声称每个一级标题必然是一篇。Reader元信息与正文分开，保留原响应快照。HTML目录链接作为PageLink候选留存，用于P3/P6沿用户已选标题读取，P2不自行遍历。

仅有标题／链接、或容器含明确“阅读全文／Read more”入口的文章卡片，保留候选但标needs_inspection；不据article标签就认定已取得全文。HTTP成功与文章成立分开。200登录／导航页保留快照并标记non_article；无法确定的正文保留needs_inspection。短文不按字数拒绝，真实可读段落不因缺文章标签机械拒绝。

### 4.3 资源与失败

每来源最多8MiB，来源清单JSON最多8MiB，单清单最多32项来源引用；它们是读取资源边界，不是风格质量标准。文件和HTTP都增量读取到上限＋1才判断，不静默截断；超限、无效UTF-8、不支持格式、空文和网络失败分别留reason。DOM遍历深度限制128，超限整源识别失败，不能交出截断正文。

P2不读取任意作者身份、不自动搜索替代文章。URL只接受HTTP/HTTPS且无内嵌认证信息；网络错误只记录安全类别／HTTP状态，不保存客户端凭证或原始错误URL文本。正文指令只留作材料。

### 4.4 输出、保存和完整性

SourceCollection包含schema/method/id、原SourceRequest、base_dir、collected_at、SourceRecord列表及各路径attempts/snapshots/candidates/links。原响应快照与正文哈希可核对，完整记录形成SHA身份。来源采集成功不等于语义质量验收。

每来源状态区分extracted、needs_inspection、non_article、failed、unsupported；collection结构判定为ready、needs_inspection、needs_input或blocked。ready仅表示选文和数量的程序边界满足，仍需P3的目标／声音范围判断。

可读但语义边界不明时先needs_inspection，交Agent自己检查，不直接变成用户追问；完全无可读材料才blocked。未选定的多文来源、独立正文超过10、明确篇数不足、未获准的部分失败均needs_input。明确allow_partial可只取单篇来源中有效正文，但不能越过expected_articles。重复保留可见标记；不因重复就再问一次，除非明确独立篇数受影响。

collection以完整不可覆盖JSON发布至工作区source-collections/<id>.json。恢复／查看只读取快照并验证身份与正文／原响应哈希，不重新获取来源。export-texts仅在ready时写出与已有learn兼容的articles/note正文包，使用不可覆盖发布；多文／失败范围未解决不能导出。P2不直接创建学习任务，不产生模型费用。

### 4.5 开发CLI

沿现有style-draft增加：collect <sources.json>、collection <collection-id>、export-texts <collection-id> <new.json>。采集结果展示状态、候选标题、重复与原因，不把网页原始大段内容倒进终端。用户可明确使用已有learn命令学习ready正文包；这条开发链不是P4的自然语言入口。

P3再补受用户范围决策约束的select_articles工具；选择只在冻结快照上执行，不重新抓页面。P2的多文识别和阻断不冒充P6的完整选文产品交互。

## 5. 后续工具与版本桥接（P3–P6实施）

collect_sources返回collection_id及摘要；inspect_sources读取候选与来源；select_articles记录明确选择及范围修订；learn_texts将完整选定正文与准确base交给现有learning_job；preview/save/diff复用工作区。保存已有意图时不重复确认，没有名字则先返回未命名候选。

Writer使用指定候选／版本的spec内存值，保留已有styles/*.md读取兼容；主会话持有SelectedStyle引用，不能把临时候选伪装为已入库风格，也不为写一篇草稿自动覆盖正式styles。具体REPL字段和迁移在P4前按实际调用方定稿。

## 6. A01–A24设计走查（非运行通过）

|案例|设计中的路径与边界|实施／验证阶段|
|---|---|---|
|A01|单URL取得一篇正文→ready→提交学习，0澄清|P2/P3/P5|
|A02|两文件独立快照，去重后两篇且数量满足→ready|P2/P5|
|A03|主Agent识别使用已有风格，不创建学习任务|P4|
|A04|委派保留写作需求；候选／版本完成后仅spec交Writer|P4/P5|
|A05|多篇候选→needs_input；选文后以同一快照继续|P2识别，P3/P6交互|
|A06|页面链接保留；已指定标题可跟随，纯导航无正文不能ready|P2识别，P3/P6选目标|
|A07|明确5篇中两失败→needs_input，不导出3篇冒充全部|P2/P3|
|A08|allow_partial且无明确数量冲突→列失败与实际范围，继续有效材料|P2/P3|
|A09|规范哈希去重并保留所有来源，独立篇数不足才阻断|P2|
|A10|正文集＞10／追加总数＞10不能学习；不能静默截取|P2/P5/P6|
|A11|用户明确综合差异声音→记录目标，不再鉴定作者身份|P3|
|A12|最多两次主动澄清后保存等待原因，不出现第三轮|P3/P4|
|A13|同task/revision/question持久化恢复，不再生成同问题|P3/P4|
|A14|原学习请求已发未收保留未知，沿现有显式重试边界|P5/P8|
|A15|collection查看／导出不取网络，学习恢复不再发已完成请求，保存不重复版本|P2/P5/P8|
|A16|正文只进入材料字段，确定性工具验证来源、计数与权限|P2/P3|
|A17|保存意图随委派保留；无名先交候选，不靠命名增加澄清|P3/P5|
|A18|现有工作区阻断过期基准，保留候选说明冲突|P5/P8|
|A19|P2不支持DOCX/PDF即明确说明；P6补文字提取，扫描件不猜正文|P2/P6|
|A20|批量采集占一次决策，12步／2次无进展收束，等待不调用模型|P3/P8|
|A21|复用现有来源核验但仍逐项检查限定与表达功能|P7-S|
|A22|10篇结构可处理不等于语义通过；P7补当前方法真实质量|P2数量／P7-S|
|A23|成稿单独作事实核对，保留原稿和一次修订差异|P7-W|
|A24|revision隔离迟到回执，原费用回原任务，不采用到当前任务|P3/P4/P8|

走查结论：保留默认1轮／最多2轮、12决策步等首版值；批量工具可避免10篇正常输入挤爆决策数。所有值仍需P3运行检查，当前不声称用户体验或模型语义已验收。

## 7. P1/P2实施与验证范围（历史阶段）

本轮按P1→P2顺序实施：先设计，后新增输入模块与开发命令；不改变旧learn/REPL、原学习提示词、生产配置或styles。单元测试内嵌各模块，I/O及HTTP夹具用临时目录和本地端口，检查正文边界、失败不丢、去重、数量、冻结恢复与不可覆盖；模块各≤500行。完成后cargo test --lib、构建及相称CLI回归、git diff与本地提交，实际状态回写PRD／行动计划。

P3–P8的合同是后续实现依据，不记为本轮已实现。新增解析依赖只为P2所需，不引入付费抓取服务、图框架或新的生产配置。

## 8. P3实施合同（2026-11-03，先于代码）

### 8.1 本阶段终点与入口

P3实现持久化的材料决策Agent，终点为ready_for_learning（已明确可交给学习工具的正文集合），不冒充candidate_ready。实际Pro学习／保存／Writer交接留在P5，主会话唯一stdin接入留在P4。开发命令为agent-start <delegate.json>、agent <task-id>、agent-resume <task-id> [--retry-uncertain]、agent-answer <task-id> <reply.json>、agent-cancel <task-id>。

Delegate携带user_request、request（P2 SourceRequest）、target_titles、all_articles（用户明确整页全部的授权，默认false）、base、save_name、followup_request、budget_micro_cny。可选字段默认空；任务预算默认0.25元、最大1元，实际按调用前预留约束。基准版从现有工作区校验并保留，追加总数1–10的硬规则不会交给模型自行放宽。

明确材料先批量采集；范围清楚且没有内部待检查项时按已给目标直接准备正文，0决策模型请求。这是少打断和节省调用的快速路径。复杂情况由Flash动态选择inspect、resolve、select、clarify、escalate或stop；不是每个固定动作都再问一次模型。

### 8.2 决策与工具边界

inspect读取冻结来源候选和页面信息；默认观察只含摘要，返回内容注明是否片段，不伪称完整阅读。resolve要求先检查并给出可定位证据，可将一项来源判为可学文章或非文章；提供的TXT/MD按多个一级标题分段时，可依据内容恢复为该文件的一篇完整文章。HTML多篇不能借此合并成一篇。

select只能引用真实候选ID，去重后1–10篇；普通清楚集合可全部采用。多文来源的子集选择须来自用户明确ID选择或Delegate.target_titles的唯一匹配，模型不能无依据挑第一篇。失败来源不能因resolve或select自动忽略；allow_partial和expected_articles仅由原请求或用户回复修改。篇数约束、追加保留旧文及十篇上限均由程序核对。

正文标为needs_inspection时先由Agent检查，不直接问用户。没有可用正文时明确blocked；未知材料不强行宣称可学。第二轮clarify须在用户已回答后才能发生，同一准确问题不重复；全任务最多两轮主动追问。连续两次无进展、12次决策或已用过的一次Pro升级都由程序限制。

### 8.3 状态、回答与恢复

AgentPlan冻结方法、原委派、提示词和端点身份。AgentState保存当前输入修订、来源集合引用、内部检查结果、选择、用户回答、提问次数、决策次数、无进展计数及Pro升级标记。状态新增ready_for_learning，并区分waiting_input、blocked、failed、cancelled。

以带父身份的不可覆盖状态快照为权威日志，发布快照前持短状态锁并比较预期身份。执行锁阻止两个run并发，但不阻止用户取消。模型与工具等待期间不持状态锁；迟到结果保留回执，只有仍匹配task/revision/待执行步骤才应用。

每步先保存pending，再调用模型或工具。决策请求使用既有learning_transport的意图／响应日志，恢复复用同请求回执；未知付费请求不自动重发。批量collect使用该步骤独享工具工作区，已发布来源集合可在重启时找回，避免仅因主状态未推进就再次抓取。

用户回复携带expected_revision、question_id（主动补充可为空）、text与可选scope变更。scope可明确选文、改变期望篇数／部分许可或补充／替换来源。输入变动增加修订并清除过期内部判断／选择，旧集合和回执保留；提问及决策预算不重置。同一回复重复提交幂等，旧问题或旧修订的新回复不能串入当前任务。

达到澄清边界后保存blocked原因，不输出第三个问题；用户主动补充仍可更新同一任务。等待和已完成恢复不调用模型。保存名和后续写作要求始终保留，不因未命名增加追问。

### 8.4 模型与费用

简单决策固定mimo-v2.6-flash，必要时最多升级一次mimo-v2.6-pro；JSON动作最大2048输出tokens，thinking disabled。复用已有客户端的官方／本地端点校验、请求体、回执与未知重试合同，不存API凭证。

普通开发命令的任务账本独立位于该任务目录，调用前按现有核价费率保守预留，收到回执后结算；缺usage保留预留并停止。研究验证通过注入同一决策合同接ResearchStore，统一计入原100元账本，P3全部验证合计≤1元，不把任务本地估算再重复记进总账。真实调用前复核官方费率及端点。

### 8.5 验证范围

先以可注入决策器和本地HTTP验证：正常0模型／0追问、未知材料内部识别、多文明确选择、部分失败与明确篇数不能被模型改掉、两轮上限、问题／回答恢复、输入修订、取消后的迟到结果、无进展及Pro升级上限、请求身份和未知费用。随后只做少量Flash真实复杂决策检查；不重跑风格质量矩阵，不改旧learn/REPL默认。

P3真实小试固定四个合成材料任务：两篇清楚来源直接就绪、两篇独立文章需选一篇后用户指定第二篇、同一连续文章的两个章节内部恢复、不明确选择经有限澄清收束。每任务最多0.25元，合计最多1元；付费请求统一留在ResearchStore账本。未收回执但已有费用预留时不隐式重发；任务链接、冻结输入、原始动作和所有状态留存。复跑只检查已有状态／回执，不重新抽样；报告区分协议通过、实际模型行为与未接入的P4/P5。

2026-11-03 P3首次真实小试留证：四个合成材料场景中3个符合预期，章节恢复因重复inspect及漏tool动作名而受阻，7个Flash响应占用0.011247元。修正先于后续代码：观察显式列已检查来源／窗口；动作JSON保留严格校验，同时反馈具体缺失字段；提示词展示完整动作对象。首轮不改写通过，保留原始回执与failure-review.json。相同来源／用户目标只复验一次，合计1元预算扣除首轮占用，不以放宽工具／篇数校验换通过。

2026-11-03 P3合同修正：real-02仍为3/4，章节场景两次合法resolve被“必须先另行inspect”拒绝；实际观察已包含全部短文正文，额外读取门槛与已提供材料不一致。替换先inspect的机械前置条件：resolve可依据完整呈现在当前观察的来源候选，否则必须先inspect。完整性由程序按实际摘要限额与excerpted=false核对；长文片段、空候选仍不能绕过读取。逐字引文、用户选文、HTML不合并和数量硬边界不变。real-01/02失败均保留；仅最后针对相同章节材料做一个真实复验，不重抽其他三场景；1元合计扣除两个旧run占用。

## 9. P3实际接口与验证入口

P3已完成材料任务层，源代码入口为learning_agent（编排）、learning_agent_types（合同）、store（不可变状态链）、scope（选择及范围门槛）、tools（采集／观察／回答）、model（回执与费用）、cli（开发命令）。每模块少于500行；复用LearningWorkspace和learning_transport，不新增图框架或独立stdin。当前合同以第8节及两次有据修正记录为准：完整显示的来源候选可直接判断，片段先读取。ready_texts返回原始完整正文，不能把预览片段当学习输入。

启动样例（仅开发入口，文件路径相对委派JSON目录）：

```json
{"user_request":"学习这两篇文章的综合写法","request":{"sources":[{"kind":"file","path":"甲.md"},{"kind":"file","path":"乙.md"}],"expected_articles":2,"note":"保留两篇范围"},"save_name":"阅读笔记的写法","followup_request":"之后按这个写法写一篇新文章","budget_micro_cny":250000}
```

```sh
cargo run -- style-draft --root /tmp/cw-agent-demo agent-start /path/to/delegate.json
cargo run -- style-draft --root /tmp/cw-agent-demo agent TASK_ID
cargo run -- style-draft --root /tmp/cw-agent-demo agent-answer TASK_ID /path/to/reply.json
cargo run -- style-draft --root /tmp/cw-agent-demo agent-resume TASK_ID
cargo run -- style-draft --root /tmp/cw-agent-demo agent-cancel TASK_ID
```

回复结构：

```json
{"expected_revision":0,"question_id":"当前问题ID","text":"选择乙这一篇","scope":{"article_ids":["s001-a002"],"expected_articles":1}}
```

显式未收回执请求重试须加--retry-uncertain，最多三次，旧预留保留；普通恢复不自动重发。已blocked且用户主动补充时question_id可为空。P3无自然语言回复解析，此JSON边界由P4接主会话；save_name／followup_request目前只保留，P5才执行相应交接。

验证：cargo test --lib；scripts/verify_learning_agent.py；旧scripts/verify_learning_jobs.py与verify_repl_terminal.py。research learning-agent <run>是受统一100元账本及额外阶段限额约束的付费验证入口，不属于常规自测。真实证据为learning-agent-real-01/02/03，前两个partial保留；全库309通过／10忽略、本地9场景通过、最后章节有界复验通过。下一阶段P4及P5尚未实现，完整产品／风格语义不在此宣称通过。

## 10. P4主会话实施合同（2026-11-05，先于代码）

### 10.1 开发入口与角色

style-draft [--root 工作区] chat使用既有Repl与唯一repl_input读取器，独立开发会话文件在工作区chat-session，旧默认入口不切换。新增WaitingForLearningAgent状态；Repl仅保存ChatRef（工作区／会话ID），领域材料仍在agent-tasks。父会话保存原输入、活动任务ID、待执行解释／交接及暂停状态，不复制领域问题计数。P4结束于ready_for_learning，组合任务原始写作请求留在Delegate.followup_request。已有风格写作交回既有Generate，不创建学习任务。

### 10.2 主会话日志与恢复

父会话使用带父身份的不可覆盖状态快照；先冻结输入、原话、模型提示词和端点，保存pending，才做解释／委派／回复应用。任务创建链接先落盘才启动领域模型；中断后沿同一pending与任务恢复。读取状态必须验证身份链，不因文件缺失／损坏重新新建。待模型输入和回执复用learning_transport；普通恢复不重发未知请求，显式重试沿最多三次且保留旧预留。领域任务回执与费用仍归原任务。

Repl进入处理前保存ChatRef，中断保留父pending和已有领域任务。恢复只展示已存在的待答问题，不再次生成；用户说继续才继续未完成动作，waiting／ready时不调用模型。取消或修订后迟到的旧动作必须按父快照／任务修订核对，不能应用到新任务。

### 10.3 输入解释与用户负担

清楚的URL／文件和正文标记、精确标题／选文编号、查看／继续／取消／暂停等走本地；复杂学习意图或自由回复由Flash解释成严格JSON。主会话解释与领域判断分开：主会话解释用户明确范围，领域Agent判断文章。来源、正文和嵌入命令不可改变工具权限；解释器只能引用原话中的文件／URL／正文或真实候选ID。保存名／基准名须来自原话；篇数、部分许可、整页全部等控制需有原话支持。模型不能通过解释追加授权。

没有来源的学习需求也建领域任务并问一次文章材料，计入P3同一两轮预算；后续普通不明确答复保留后blocked，不另起父级追问预算。主会话不重写子Agent问题，状态查看不重复输出问题正文。明确材料／数量／标题不重复问。

同时只有一项活动学习；未明确替换的新请求保留并说明已有任务，不私自覆盖。明确替换或先取消可建新任务。暂停保留任务，普通输入不自动恢复。改目标／补来源通过P3同任务revise增加修订；waiting、blocked、ready或失败请求均可有明确变更，提问／决策总计不清零。

### 10.4 P3边界补充

Agent准备允许空来源的有效委派，不调用采集；初始waiting_input仅有文章材料问题。空来源的回答仍无材料时blocked并保留回答。追加／选文等原硬规则不变。新增显式revise用于ready／active／failed的有效目标或材料变更，禁止取消后复活；普通answer仍只接受当前问题与等待修订。缺失任务／旧版本身份冲突不建立新任务代替。

### 10.5 模型、验证与费用

父解释器mimo-v2.6-flash，thinking disabled、JSON对象、最大2048输出tokens；共用P3客户端的身份、回执和费用实现，生产开发任务账本与研究账本不重复收费。真实P4检查通过注入ResearchStore统一记入100元；所有P4小试及失败合计≤0.5元。先核价再请求。

本地重点验学习／使用／组合、原始写作要求、来源不编造、明确及自然语言回复、两轮边界、无来源补充、状态／问题恢复、暂停／取消／目标改动、一活跃任务和迟到不串。真实终端测试使用隔离工作区与本地服务，旧单篇／生成／局部修改回归按影响执行；真实小试不证明Style质量，不调用P5或Writer。阶段结果及费用回写文档，失败和最终针对修正分别保留。

2026-11-05 P4输入边界补充（实现前）：每条会话输入最多8MiB。当前仅有一个不可用来源且没有可用候选时，用户随后直接提供的文件／网址／正文作为该来源的替代材料；明确“再加”才追加，旧来源快照保留。多来源或已有候选不猜归属。明确“取消当前学习，改用已有风格写作”先取消再处理原写作要求；“取消学习／不学了”直接取消，不交模型误作普通回答。

2026-11-05 P4失败来源交互补充（实现前）：所有给定来源均失败／非文章时，P3不止留下blocked技术状态；若尚未问过材料修正问题且两轮预算仍有余量，集中要求一次可用材料，计入原领域任务预算。重复修正仍无正文或到达两轮即收束。工具已能确认没有正文，不为这一次说明额外调用模型；父会话原样转交。

2026-11-05 P4收尾审查补充（修正前）：本地选文须排除被否定的编号、ID及标题，明确列出的多个选文不能只取第一个；数量冲突继续交领域范围校验。已有风格的多个提及／否定不走单名称快速推断，交Flash解释且所选名称须获原话肯定支持。增加对应反例验证，不重抽已完成真实用例。

### 10.6 P4实际实现与验证（2026-11-09）

本节已在style-draft chat落地。Repl保存兼容默认空的ChatRef并使用原stdin；父状态／待执行输入在chats/<id>的身份链内，领域材料／问题仍在agent-tasks。处理前先保存引用，恢复不重建任务；原始输入、保存名、后续写作需求及精确问题身份保留。输入文件相对会话启动目录，冻结后恢复不换基准。

学习／已有风格使用／组合路由、正常零问、材料缺失／失败的一次领域问题、替代材料、暂停／继续、取消／明确替换／同任务目标修订均有运行证据。12新增测试及321库测试、9新终端／15模拟请求和旧回归通过；真实固定四场景只3个Flash请求，复跑0请求。查阅研究reports/learning-chat-2026-11-09.md及runs/learning-chat-real-01。P5学习／候选／保存／Writer未调用，P4不是质量或正式采用验收。

2026-11-09 P4原话／正文边界复核（修正前）：正文标记之后的文件路径、URL仅是文章内容，不能由解释器提升为新来源；文件／URL来源必须能定位到正文之外的用户要求。补正文注入反例，保留现有字段／请求及两轮边界。

2026-11-09 P4篇数边界复核（修正前）：明确的11／12等超限数量须完整识别并拒绝，不能从“11篇”取出“1篇”；中文“十一篇”同样保留超限意图，“第十一篇”仍仅为序号，不当篇数。补数量反例，十篇上限不变。


## 11. P5学习／版本／Writer交接合同（2026-11-10，先于代码）

新增learning_flow编排层复用P4材料Agent，P4的run_with材料接口保持可单独验证。父会话状态新增可缺省delivery引用，绑定task_id/revision/job_id和候选/版本；旧会话可读。学习任务必须在发送请求前写入父状态，正文来自ready_texts（包含追加旧正文），Pro配置和JobPrompts冻结在job。恢复先查绑定，修订后不得采用旧job；运行锁与父状态CAS校验隔离取消/迟到结果。

完成候选后展示完整spec、来源数量与差异入口；无名称可等待主动操作而不占材料澄清预算。父状态保留原保存意图，执行保存前校验当前绑定；工作区save先查同候选同名的既有版本返回它，其他冲突仍拒绝。保留候选、旧版及费用。对话命令：/preview、/diff、/save <名称>、/versions <名称>、/show <名称> <版本>、/use-version <名称> <版本> <写作要求>，自然“保存为X”“预览候选”“查看差异”提供明确快路径。

组合写作提取control中的后续写作片段；原文在前时末尾独立段作为control，前文整体为正文，不扫描正文路径。正文在后时沿原标记控制界面；无标记但指令在首行且后接段落也可界定。正文不含末尾命令。只有明确写作意图才触发Writer，独立保存不触发。无名候选可用于这一次明确写作，后续通过命令另存。

Writer使用现有generate的system/render提示构造函数；Flash固定路由；大纲和正文各有learning_transport逐阶段回执与最多三次显式未知重试，不走旧自动重试包装。Writer输出进入既有WaitingForFulltext编辑/确认流程；父交接结果留存，重启不重复请求。Writer记录只含纯spec、写作要求及输出，不含原文章/分析/复读；阶段完成可查看候选但不反复重发写作。

验证先以合成文章和本地HTTP逐字段检查，再用固定真实单篇及追加双篇小试；2元总限额统一ResearchStore记账。P5验证接通和隔离，P7再判断风格含义与成稿事实；两类结论分开。

P5实现补充：候选完成后，明确新学习/追加请求建立新的材料任务，修订目标仍沿旧task递增revision；父状态的可缺省delivery和selected_writer用skip_serializing_if保持旧P4哈希链可读。自然“用X风格写…”可读工作区最新明确名称；若旧styles与工作区同名，先提示使用指定版本命令，不静默选一方。/use-candidate <要求>可直接使用未命名候选。Writer回执与学习回执各自缓存，取消后下一学习阶段不再发送。

### 11.1 P5实际验证与范围

2026-11-11完成，入口learning_flow.rs、learning_flow_commands.rs、learning_flow_writer.rs及REPL桥接；底层save幂等修正。6新测试/327通过/10忽略；5真实PTY/39本地请求及旧相关回归通过。固定真实单篇→追加双篇→指定旧版写作11回执，最终重放0请求；8Pro/3Flash新增0.160166元。候选/版本/来源和Writer原稿、事实问题均留研究目录；P5不证明说明语义/10篇或Writer事实质量。细节见P5阶段报告，同次实现提交由git log -- src/learning_flow.rs定位。
