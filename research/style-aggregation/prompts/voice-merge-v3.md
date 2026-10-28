根据已逐字定位的逐篇观察，写一份可供 Writer 在无关题材使用的详细声音说明。每条观察仍只是候选：先判断引文能否支撑其操作，不能靠相同词语或标点就合并。只有一篇依据时标明局部性；两篇含相同机制才称反复出现。两篇写法不同但可由题材/材料条件解释时写清变体，无法判断的保留未知。一篇没出现某技法不等于反例。

给 Writer 的 spec 要把整体声音、具体写作动作、适用条件、强度、互相让位、常见误用和成稿检查讲透，不用固定栏目或篇幅配额。绝不写入原文的故事、原句、人物、物件、地名、独特短语或作者名；引文与来源只放在规则审阅字段。不要规定每篇必须先有亲历、几段叙事、几次反问或特定结尾。题目事实与必写信息优先；材料薄时靠已有事实的组织和判断保持声音，不编现场。

当输入有 previous 时，你是在增量更新。逐条说明旧规则是 retained、refined 还是 retired，并给出具体依据；不静默丢失，不因追求表面一致而保留过度概括。新增文章没改变核心时可以写 retained。规则ID只用于审阅，不进 spec。

只返回 JSON：{"spec":"给Writer阅读的详细Markdown","rules":[{"id":"r1","instruction":"抽象可执行机制","when":"适用条件","scope":"local 或 repeated 或 conditional_variant","from_cards":["a01:c1"],"misuse":"误用与纠正"}],"changes":[{"prior_rule":"r1","action":"retained 或 refined 或 retired","reason":"旧规则为何保留/改变/撤销","new_rule_ids":["r1"]}],"uncertain":["尚无充分依据的写法"]}。输入无 previous 时 changes 为空；输入有 previous 时每条旧规则必须恰好交代一次。retired 的 new_rule_ids 为空，其他动作指向本轮规则。每条规则至少有一条观察来源；repeated 至少涉及两篇，local 只涉及一篇。不要把审阅用的引文复制到 spec。
