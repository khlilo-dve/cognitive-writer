# 风格学习子Agent技术设计

2026-11-03 · v0.1 · 需求依据：[PRD](../PRD.md) SL-01至SL-16；[行动计划](../plans/2026-11-03-style-learning-agent-plan.md) P1–P8。本设计先于新增实现落盘。用户已指示“按照计划开始开发”，原P0仅文档限制已由该指令结束；不需要重复确认已批准方向。

## 1. 复用边界与架构决定

保留Rust主程序和REPL唯一stdin读取器。主Agent负责顶层意图与对话，子Agent是同进程内持有独立任务状态、模型决策及受限工具集合的领域角色。不会把每个节点做成独立Agent，也不先引入通用Graph Runtime。

复用learning_job的完整正文学习／原文复读、LearningWorkspace的候选与版本、learning_transport的请求身份与未知费用边界。P2只做输入工具，不能称子Agent已经实现。旧learn默认路径不改。

网页解析新增scraper 0.29（以Cargo.lock实际解析版本为准），用于HTML DOM与CSS选择；不复用旧strip_html_tags作为新输入链的文章判断。官方API依据：[scraper](https://docs.rs/scraper/0.29.0/scraper/)。网络沿用reqwest 0.12。

## 2. 主子Agent交接合同（P3/P4实施）

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

结构化HTML先看独立article／articleBody／微信公众号正文容器，再看main／明确标题与正文段落。提取排除script/style/nav/站点header/footer/aside/form等非正文，保留article内部标题头部、隐藏节点，保留段落与行内连接；没有明确正文边界时返回needs_inspection，而不是接受整页去标签结果。article嵌套只保留外层，多个同级article产生多个候选，不默认全选。

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

## 7. 实施与验证范围

本轮按P1→P2顺序实施：先设计，后新增输入模块与开发命令；不改变旧learn/REPL、原学习提示词、生产配置或styles。单元测试内嵌各模块，I/O及HTTP夹具用临时目录和本地端口，检查正文边界、失败不丢、去重、数量、冻结恢复与不可覆盖；模块各≤500行。完成后cargo test --lib、构建及相称CLI回归、git diff与本地提交，实际状态回写PRD／行动计划。

P3–P8的合同是后续实现依据，不记为本轮已实现。新增解析依赖只为P2所需，不引入付费抓取服务、图框架或新的生产配置。
