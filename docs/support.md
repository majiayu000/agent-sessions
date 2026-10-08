# 编程 agent 支持清单

核查日期：2026-10-08。本清单记录**具体日志结构**，不是对某个品牌全部历史版本、
未发布版本或全部能力的保证。Agent 是写入日志的客户端；GLM、DeepSeek、豆包等模型
本身不是新的日志格式，接入这些模型的客户端按客户端格式解析。

本轮范围为现有 26 个客户端；以下矩阵反映当前工作树，新增内容尚未发布。
后文保留之前的验收记录，最新结果见文末「本轮补齐后的最终验收」。格式回归通过
不代表每个客户端都已完成真实新会话验收，缺少的证据仍明确列出。

## 已实现的来源

下表的格式适配有独立语义回归测试；新增适配依据公开源码、官方文档、发行包及
本机真实日志。回归测试内容均为合成数据。实现适配、读取已有真实记录、运行客户端
生成新记录是不同的验收证据，不能互相替代；每项的实际状态见最后一列。
真实会话、凭据、个人路径没有复制进仓库。源码 main 分支的观察不等于已发布版本保证。

| Agent | 读取来源 | 已提取内容与边界 | 实际客户端验收 |
|---|---|---|---|
| Claude Code | `projects/**/*.jsonl` | 原有消息、工具、用量和元数据读取；历史索引与标题索引仍受原有契约约束 | 已有真实日志抽读；2.1.281 新会话被服务端 403 阻塞 |
| Codex | sessions / archived_sessions JSONL | response_item、早期 user_message/无 payload.type 的消息、token_count、token_usage_record；根据 session_meta.history_mode 自动选择分页 item_completed，避免正文重复 | 0.160.1 新会话实际运行，正文及工具内容校验通过 |
| Gemini CLI | 会话 JSON；session JSONL | 原生正文、toolCalls、tokens；JSONL 回放 `$patch`、`$set`、`$rewindTo`，不返回被替换的旧正文或已撤回消息 | 合成样例；尚未运行客户端验收 |
| Qwen Code | `projects/*/chats/*.jsonl` | ChatRecord.message.parts，functionCall / functionResponse，usageMetadata；systemPayload 的正文/显示文本；其他系统结构按实际诊断判断 | 合成样例；尚未运行客户端验收 |
| Kimi CLI / Kimi Code | 旧 wire/context JSONL；Kimi Code v2 wire journal | 旧 wire/SubagentEvent 分片合并并保留父工具关联；v2 合并 step 内内容及工具、延迟注入，按 agentId 回放 undo/clear/新旧压缩；保留原始计费用量，不补造中断工具结果 | 已有真实 v2 日志逐字段核对；新增上下文控制回归通过；本机 0.29.0 新会话缺少 OAuth 登录 |
| Pi | 会话 JSONL | 快照沿最新叶节点选择 parentId 分支，应用 context_edit 和最新 compaction；兼容 v1 线性/index 压缩；物理用量独立保留；手动 bash、custom、分支摘要与 details 原样保留 | 合成样例覆盖分支、编辑、压缩、v1、手动工具及用量；尚未运行客户端验收 |
| GitHub Copilot CLI | `session-state/*/events.jsonl` | user / assistant / system、execution_start / complete、元数据；attachments、reasoning 和 toolRequests 保留 Content；不把 toolRequests 重复计为调用；assistant.usage 通常是临时事件，未落盘用量保持未知 | 合成样例；尚未运行客户端验收 |
| CodeBuddy CLI | Claude 兼容 JSONL；SDK 原生 item / SessionStore payload envelope | message/function_call/result/reasoning、providerData.usage；按 uuid 补写，回放 resend-fork-notice 和 /clear，保留物理用量；旧共享 id 不误去重，分支 id 歧义明确失败；每次读取一个会话/lane 文件，不猜测另存的 lane 指针 | 核对官方 2.161.4 包；两种格式、分支/补写/clear/歧义回归通过；尚未运行客户端验收 |
| iFlow CLI | `.iflow/projects/*/*.jsonl`，核对官方 0.5.19 发行包 | 原生 Claude 形状的消息、工具、用量、元数据；结构化 tool_result 保留为 ToolResult.output | 合成样例；尚未运行客户端验收 |
| OpenCode | export JSON、原生 SQLite、旧 storage/session/message/part JSON 树 | 正文、reasoning/file 等原生 part、工具和消息用量；旧文件树从 session 文件关联读取，按原生 id 排序，校验会话/消息关联并保留全部文件来源；step-finish 用量镜像不重复累计 | 1.18.27 新会话实际运行；JSON 与 SQLite 一致；旧树按官方 1.0.0 源码回归，与 export 事件一致 |
| Cline | 原生 `api_conversation_history.json` 数组 | Anthropic API 消息、工具、ts；此项不覆盖所有新版 Cline SDK/CLI 存储协议，也不把 ui_messages.json 的 UI 操作当聊天正文 | 合成样例；尚未运行客户端验收 |
| Roo Code | 原生 `api_conversation_history.json` 数组 | Anthropic API 消息、工具、ts；reasoning/媒体保留为 Content；无自动压缩分支过滤 | 合成样例；尚未运行客户端验收 |
| Goose | 官方 session JSON 导出或原生 SQLite | conversation、toolRequest/toolResponse、metadata.usage；SQLite sessions/messages；不把 session 累计用量重复算为每条消息用量 | 合成样例；尚未运行客户端验收 |
| Continue | `.continue/sessions/*.json` 会话对象 | history[].message，toolCalls / toolCallId，消息 usage；session 累计 usage 不重复输出；contextItems 保留为 Content | 合成样例；尚未运行客户端验收 |
| Cursor | 原生 globalStorage/state.vscdb | composerData、按 header 顺序的 bubbleId、ISO 时间戳、toolFormerData；thinking/images/attachedFiles/context/richText 保留 Content；tokenCount 未确认为账单用量，不输出 Usage；缺失/加密正文及未识别工具会报 unknown | 已有真实数据库抽读；未新建 IDE 对话 |
| Grok Build | `updates.jsonl` ACP journal；显式 `chat_history.jsonl` 快照 | 标准及 `_x.ai/session/update`；工具补写合并，保留全部来源及 rawOutput；仅 turn_completed.usage，不重复累计镜像；工具名取原生元数据；chat 快照保留 system/user/assistant/tool_result/reasoning，缺失用量不补造；不重建 journal 撤回后的上下文，自动发现只选 journal | 1.0.46 新会话实际运行；预定正文及文件输出、原始字段对照通过，错误预期会失败 |
| Cline CLI | `sessions/*/*.messages.json` 会话快照 | messages[] 中 API 正文、工具、毫秒时间与 metrics；内部 `{query,result,success}` 结果保留原始数组，独立于 Cline IDE 的 API 历史文件 | 3.0.68 已有真实日志逐字段核对；新会话被认证错误阻塞 |
| Hermes Agent | `sessions/session_*.json`、原生 `state.db` | OpenAI 消息、tool_calls、tool_call_id；SQLite 按原生 id 顺序、NUL+json 标记读取，恢复存在的 reasoning/reasoning_content/reasoning_details/codex_reasoning_items/codex_message_items；合并内容预算；不筛选 active/compacted 或伪造 session 汇总用量 | 已有 JSON 及三个真实数据库会话核对；新增原生 reasoning 列回归通过；新会话被 Unauthorized 阻塞 |
| WorkBuddy | 权威 `.workbuddy/projects/**/*.jsonl` | message / function_call / function_call_result；参数保留原始字符串，结构化结果、毫秒时间；reasoning、媒体引用及文件快照保留为 Content；audit-log 不另算会话 | 本机 5.3.14 全部 29 个非空会话导入，工具参数/结果逐字段核对；本轮应用内附 CLI 2.115.0 启动后 180 秒无输出，端到端失败 |
| Qoder | `.qoder/tasks/*/*.jsonl`、projects 主会话及 transcript JSONL | 旧 role/message 与新版 type/message，Claude 内容块、工具与元数据；不把镜像 transcript 与主会话合并计数 | 本机 1.21.2 全部 18 个入口读取；115 条非空正文和 215 次工具参数与独立原生记录逐字段一致；未新建对话 |
| ZCode | 原生 `db.sqlite` 的 session/message/part | 按原生 sequence 读取消息/part、正文、工具与消息用量；timeline 是模型切换元数据；不读取 model-io 请求跟踪或任务索引作为第二份正文 | 本机 3.14.1 全部 50 个数据库会话：729 条消息、949 次调用及结果与独立 SQL 逐字段一致；未新建对话 |
| Grok Bot | `sand-client-persistence/*.blob` 的 transcript replica | base32 文件名筛选会话；schemaVersion=1 的 value.entries；按厂商 renderer 读取角色/正文；附件、事件及工具轮廓保留 Content；有 id/name/status 的工具轮廓输出 Missing 参数，不伪造执行结果 | 本机 0.66.0 当前 19 个 replica 全部读取无 unknown；此前 18 个的 1041 条正文/角色独立核对一致；不保证缓存有服务端全部历史 |
| Cursor Agent CLI | `.cursor/chats/*/*/store.db` 的 meta / blobs | 按 latestRootBlobId、rootPromptMessagesJson、turns/steps 引用读取；587 个厂商 protobuf 定义；完整参数/结果、外置 contentBlobId 文本和来源、shell turn；UserMessage/ThinkingMessage 原生载荷保留 Content；不遍历不可达 blob，不导出密钥或把 context token 当账单 | 本机 2026.09.15-d2fe57e：127 条消息、1024 次调用/结果独立 SDK 对照一致；新增 326 个用户/思考载荷对照一致；本轮 CLI 2026.09.18-9a7762b 新会话工具被本机 hook 拒绝，端到端失败 |
| Zed | 原生 threads.db 的 threads.data；DbThread JSON | 原生 User/Agent 外部标签，文本、Mention、ToolUse、完整 tool_results；Image/Thinking/RedactedThinking/Resume/Compaction 保留 Content；json/zstd 且限制解压体积；仅 request_token_usage，不重复累计 cumulative；不包含 ACP 的其他存储协议 | 本轮通过内置 Agent 创建新原生线程并完成文件任务；端到端发现 raw JSON 工具参数漏读，修复后正文、参数/结果关联、顺序、用量及来源验收通过；新增 raw/tagged input 回归 |
| Warp | 原生 warp.sqlite 的 agent_tasks.task protobuf | 用户/助手正文、ToolCall oneof 参数、完整 ToolCallResult；userQuery 附件/上下文、agentReasoning 等原生载荷保留 Content；任务按数据库 id、每任务按消息顺序读取，不声明跨任务因果线性化；不累计 conversation_data 用量镜像 | 本机 0.2026.09.23.14.34.01 五个真实会话、九个任务读取；45 条消息、37 次调用、30 份结果，与独立官方 protobuf Python 解码逐字段一致 |
| Antigravity | 原生语言服务完整 GetCascadeTrajectory JSON | userInput、plannerResponse、metadata.toolCall、完整执行结果和 modelUsage；调用按原生 id 去重，保留 pending；numTotalSteps 与 steps 长度不符直接失败；旧加密 pb/新 db 经厂商服务导出，不直接猜解密；额外 generator/executor 状态不作为正文 | 本机 2.19.1 原生服务读取四个会话，共 149 步（41/6/42/60），返回数量完整；未发起新模型对话 |

所有结构化接口默认导出**文本、工具、可确认用量及原生 Content**。图片、音视频、
thinking、签名、文件附件和上下文控制以 `Event::Content { role, kind, data }` 保存原始
JSON 载荷，通过 `EventKinds::CONTENT` 选择。`Message.text` 仍只投影文本；Content
可能含同条消息的文本或工具轮廓，不能再当第二份正文/执行记录累计。外部文件、URL、
加密载荷保留引用或密文，不下载或猜解密。禁用 CONTENT 时对应省略计入 ignored。
这不是任意 JSON 全字段的无损序列化；保留原始 JSONL 字节请使用 raw reader。
工具结构化结果在 `ToolResult.output` 中原样保留，不把 JSON 字符串偷偷修复成对象。

用量字段缺失保持 None；不推测价格。尚未证明输入/缓存/推理计数重叠关系的来源使用
TokenSemantics::Unknown，exclusive() 返回 None；无法获取的用量不伪造为零。

## 使用方式与完整性

- 原有 `read` / `read_from` 仍是逐行流式 API，适用于 Claude、Codex、Qwen、Pi、
  Copilot、CodeBuddy、iFlow，以及 Kimi context。文档/数据库 agent 传入此接口会明确报错。
- `import_session` / `import_session_from` 是完整前缀的快照读取接口。Gemini 补写/回退、
  Kimi 分片和上下文控制、Pi 分支/编辑/压缩、CodeBuddy 补写/分支/clear 先回放再返回。
  用量按物理账本保留，不因上下文撤回而消失。流式接口仅输出物理记录，无法撤回事件。
  Kimi 不补造 SDK 为模型拼接的中断结果或省略提醒，因此不是模型请求的逐字节重建。
- JSON 文档使用 JSON Pointer；JSONL 保留物理位置；数据库保留表名和行键。
  合并消息记录全部贡献来源，不伪造数据库的字节偏移。OpenCode 旧文件树使用
  `ImportSource::File { path, pointer }`，累计预算覆盖所有读取文件；不接受单文件
  stop_at_byte。多文件树不是原子事务，读取中被客户端改写时由调用方安排稳定快照。
- SQLite 使用只读连接和读取事务。`list_database_sessions` 枚举 ID，
  `import_database` 读取指定 ID；数据库读取不接受 stop_at_byte。
- `ReadSummary::is_complete()` 仅描述读取完成；`is_supported()` 还要求没有 unknown。
  `ignored_types` 是按支持契约明确省略的内容，仍须按应用用途检查。任何一个谓词都不
  证明品牌的全部功能、多模态还原、跨文件去重或账单精度。
- 解析/IO 错误保留为 ImportError 的 Read/Stream 包装，不转换成成功的空会话。
  AllowIncomplete 仅容忍 JSONL 最后一个未换行且因 EOF 截断的 JSON；JSON 文档严格解析。
- `discover_directory(agent, root, filter)` 支持各原生文件名/扩展名，并跳过 symlink。
  默认 Roots 的自动扫描仍仅针对 Claude/Codex；其他来源由调用者给出原生目录。
  Kimi 自动目录扫描只选 wire，避免同时读 context 导致重复。SQLite 文件可包含多个会话。
- 历史输入索引和标题索引 API 只实现 Claude/Codex；其他 agent 调用不伪装为已支持。

例子只打印计数与诊断，不打印聊天正文：

```sh
cargo run --locked --example inspect_session -- gemini /path/to/session.jsonl
cargo run --locked --example inspect_session -- qwen /path/to/session.jsonl
cargo run --locked --example inspect_session -- kimi /path/to/wire.jsonl
cargo run --locked --example inspect_session -- iflow /path/to/session.jsonl
cargo run --locked --example inspect_session -- opencode /path/to/export.json
cargo run --locked --example inspect_session -- opencode /path/to/opencode.db SESSION_ID
cargo run --locked --example inspect_session -- cursor /path/to/state.vscdb COMPOSER_ID
cargo run --locked --example inspect_session -- codex /path/to/session.jsonl completed-items
cargo run --locked --example inspect_session -- grok /path/to/updates.jsonl
cargo run --locked --example inspect_session -- cline-cli /path/to/SESSION.messages.json
cargo run --locked --example inspect_session -- hermes /path/to/session_example.json
cargo run --locked --example inspect_session -- hermes /path/to/state.db SESSION_ID
```

Codex 的内容来源与用量账本分别选择：默认 Auto + TokenCount；Auto 根据原生
session_meta.history_mode 的 legacy/paginated 选择 ResponseItems/CompletedItems。
缺少该元数据时按原生协议的 legacy 默认值读取；不含元数据的分页片段须显式选择
CompletedItems；response 用量使用 Response。不要把两份同内容的结果相加。
CompletedItems 不补造没有保存的工具参数；未实现的 TurnItem 子类型会报 unknown。
FileChange 映射为 apply_patch，CollabAgentToolCall 使用原生 tool 名；缺失的原调用参数
保持 Missing。文件修改、协作调用、web search 和 clock.sleep 的完整投影对象保留在
ToolResult.output 中；子 agent 状态提示和图片投影保留为 Content。

快照导入会把所选事件放进内存。文件/行预算沿用 ReadOptions；SQLite 的行预算应用于
单条序列化记录，文件预算目前应用于数据库主文件大小。数据库 summary 不宣称字节前缀。
来源是否在读取后被改写、归档提交及跨文件身份去重由应用负责。

本机只读实测：Cursor 枚举 210 个会话，抽读 3 个非空会话，共 135 条消息、60 次工具
调用、51 条工具结果；OpenCode 枚举 347 个会话，抽读 3 个非空会话，共 8 条消息、
2 次工具调用、2 条工具结果。六次导入均完成且无 unknown。两库均超过默认 200 MiB，
验证时显式设置 max_file_bytes=None；这不是全库覆盖率或所有历史版本的证明。
另抽读 3 份本机 Codex 会话，均完成且无 unknown；Claude 抽读中出现自定义
artifact-autoreact-ledger / artifact-comment-monitor 事件，保持 unknown 提示。
JSONL 实测显式使用 16 MiB 行预算和 AllowIncomplete，未复制会话内容进仓库。

本次完成的检查：Rust 1.88 的 `cargo test --locked --all-targets`、Rust 1.95 的
文档测试、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`。
新增 39 项格式回归测试；原有合成 golden fixtures 保持通过。inspect_session 示例
也已实际运行，正常来源输出计数和诊断，未知类型示例以非零状态退出。

追加验收：在独立临时目录启动 Codex 0.160.1，让它读取仅含测试标记和中文的文件。
原生会话写出后，库提取的最终回复、工具输出与事先写入的标记逐项一致；调用、用量
均存在且无 unknown。OpenCode 1.18.27 实际生成纯文本会话并用官方 export 导出；
库提取的回复与预定标记一致，JSON 导入与原生 SQLite 导入的事件内容完全一致。
已有 Kimi Code v2 日志的 9 条非空正文片段、17 次
工具调用及参数、17 条工具结果、12 条原始用量均与直接读取原生字段的独立结果一致。

Claude 启动后实际服务返回订阅权限 403；Kimi 提示 managed:kimi-code 需要 OAuth
登录。这两次没有完成新会话运行验收。原始客户端输出留在工作区外的临时验证目录，
这里只记录计数和验证结果；下列 Grok / Cline CLI / Hermes 的追加状态单独记录。

Grok 1.0.46 在独立临时目录读取事先写入的测试文件，按要求回复测试标记及中文。
新日志的 2 条正文、1 次工具调用、1 条结果、1 条回合用量与独立原生字段读取一致，
回复及文件输出均与事先准备的预期一致。改用错误预期回复，验收进程以非零退出。
另一个已有日志的 10 条正文片段、31 次调用、31 条结果、1 条用量逐项一致。
扩大抽读到 20 份 Grok 日志后，补齐 ACP 工具结果的 content 包装变体；重测 20 份均
读取完成且无 unknown。这是有界样本检查，不是全部 Grok 会话或所有版本覆盖证明。
Cline CLI 已有日志的 283 条消息、141 次调用、141 条结构化结果、134 条 metrics
与独立原生字段读取一致。Hermes 三个非空 SQLite 会话共 23 条消息、26 次调用、
26 条结果与独立 SQL 读取一致；读取 JSON 会话也通过。对照在内存中进行，未复制
真实聊天进仓库；空会话不作为验收证据。Cline CLI 新运行返回认证错误，Hermes 新运行
返回 Unauthorized，均未取得预期回复。这些已有记录验收不替代新运行验收。
上一批原生适配验收：Rust 1.88 全目标 115 通过、0 失败、1 个隔离辅助测试 ignored。
新增这一批的最终检查结果见下方；不把上一批结果当作新增代码的验证。

## 本机发现的额外客户端

按 PATH、应用包版本、原生存储目录核查；不将“已安装”或“目录存在”算作格式支持。
Grok Build、Grok Bot 是不同客户端；Cursor IDE 和 Cursor Agent CLI 也不能合并验收。

| 客户端 | 本机证据 | 当前格式状态 |
|---|---|---|
| Grok Build 1.0.46 | 可执行命令、原生 updates journal | 已实现且新运行验收通过 |
| Cline CLI 3.0.68 | 可执行命令、独立 messages 快照 | 已实现，已有真实记录验收通过；新运行认证阻塞 |
| Hermes Agent | 可执行命令、会话 JSON、state.db | 已实现消息与工具；已有真实记录验收通过；新运行认证阻塞 |
| Cursor Agent CLI 2026.09.15-d2fe57e | 本地独立发行包 | 已实现引用 protobuf 检查点；独立 SDK 对照通过 |
| WorkBuddy 5.3.14 | 应用包 | 已实现独立 JSONL 原生格式；真实会话读取通过 |
| Qoder 1.21.2 | 应用包、含聊天状态键的 SQLite | 已实现旧 task 与新版 project JSONL；真实样本通过 |
| ZCode 3.14.1 | 应用包、session 存储目录 | 已实现原生 db.sqlite 消息/part；真实会话读取通过 |
| Antigravity 2.19.1 | 应用包及独立数据目录 | 已实现原生服务完整 trajectory 导出；四个真实会话通过 |
| Grok Bot 0.66.0 | 应用包、sand-client-persistence 的 blob | 已实现 transcript replica 的正文、附件及原生工具轮廓；未保存的参数/结果不补造 |
| Warp 0.2026.09.23.14.34.01 | 应用包 | 已实现 agent_tasks 的原生 Task protobuf；五个会话通过 |
| Zed 1.10.3 | 应用包、threads/threads.db | 已实现 DbThread 的 json/zstd；本机没有线程，真实验收待补 |

本机还存在 Qwen、Gemini、Copilot 的配置目录；未发现可用历史记录的目录仅证明配置
曾存在，不能作为原生会话验收。本表是本轮有界检查结果，不声称扫描了所有安装方式。

可复用的实际内容验收示例：先准备只含测试数据的文件和预期回复，要求 Codex 读取
测试文件并回复预定文本，运行结束后取原生 sessions JSONL。预期文件须独立准备，
不能用解析器输出生成预期。此例核对 assistant 正文、对应调用的工具输出、用量字段
存在和完整性状态；输出只有布尔结果。它不证明金额或所有工具类型的正确性。

```sh
cargo run --locked --example verify_codex_session -- NATIVE_JSONL EXPECTED_REPLY_FILE EXPECTED_TOOL_OUTPUT_FILE
```

## 尚未实现 / 不应标成已支持

这份清单是持续维护的工作范围，不能用添加 enum 名称或通用 JSON 解析代替格式验收。

| 产品/格式 | 当前缺口 / 下一步证据 |
|---|---|
| Trae IDE / SOLO | 本机没有样本；需要原生存储结构或真实脱敏导出。独立开源 Trae Agent 与 IDE 是不同产品 |
| Windsurf / Cascade | 未取得可验证的原生会话结构和样例，不能按 Cursor 的数据库结构猜测 |
| Amp | 官方有完整 JSON 导出，但本次未取得公开字段契约或可独立核对的导出样例 |
| Factory Droid | 官方有 Sessions API 与 SDK；原生 JSONL 与新版 SDK 事件仍需单独的格式适配和样例 |
| Aider | 原生日志是 Markdown，角色标记可能与正文标题冲突；不能宣称无损结构化解析 |
| Crush / Kiro / Amazon Q | 未完成其原生数据库/schema 的版本核对与解析验收 |
| JetBrains Junie、Devin、Replit、Tabnine 等 | 需要对应可访问的原生记录或官方导出契约；没有资料时保持未支持 |
| Cline 其他 SDK 协议；CodeBuddy 外置 lane 指针/IDE 数据库 | 不从现有会话文件推断独立数据库或外置分支指针；需要该存储协议的独立证据 |
| 所有客户端的新版本/未知记录 | 保持 unknown；取得最小脱敏样例后再适配，不将可能包含正文的类型直接加入忽略集 |

## 格式依据

以下是此次实际核对的来源，测试按其中的结构手写，而不是仅从实现自动生成预期：

- [Gemini 记录类型](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/chatRecordingTypes.ts)、[回放规则](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/chatRecordingService.ts)
- [Qwen ChatRecord](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/services/chatRecordingService.ts)、[原生路径](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/config/storage.ts)
- [Kimi wire 文件](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/wire/file.py)、[wire 类型](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/wire/types.py)、[Kosong 消息](https://github.com/MoonshotAI/kimi-cli/blob/main/packages/kosong/src/kosong/message.py)、[上下文](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/soul/context.py)
- [Pi 会话](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/src/core/session-manager.ts)、[消息/用量类型](https://github.com/badlogic/pi-mono/blob/main/packages/ai/src/types.ts)、[缓存排除计数转换](https://github.com/badlogic/pi-mono/blob/main/packages/ai/src/api/openai-completions.ts)
- [Copilot 官方事件说明](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/streaming-events)、[生成类型](https://github.com/github/copilot-sdk/blob/main/nodejs/src/generated/session-events.ts)
- [CodeBuddy 官方完整消息协议](https://www.codebuddy.ai/docs/cli/headless)；另外核对官方 npm 包 `@tencent-ai/codebuddy-code@2.161.4` 的 SessionStore、history-utils、transcript-branch、OpenAI SDK item 声明和独立 InMemorySessionStore 实现
- iFlow 官方 npm 包 [`@iflow-ai/iflow-cli@0.5.19`](https://www.npmjs.com/package/@iflow-ai/iflow-cli/v/0.5.19)：只下载、不运行安装脚本；核对 iflow.js 的 createUserMessage / createAssistantMessage / createToolResultMessage / saveMessage
- [OpenCode 导出](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/cli/cmd/export.ts)、[数据库读取](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/message-v2.ts)、[消息结构](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/v1/session.ts)
- [OpenCode 1.0.0 多文件存储和 ID 排序](https://github.com/anomalyco/opencode/blob/v1.0.0/packages/opencode/src/session/index.ts)
- [Cline 原生文件读写](https://github.com/cline/cline/blob/main/apps/vscode/src/core/storage/disk.ts)、[Roo API 历史](https://github.com/RooCodeInc/Roo-Code/blob/main/src/core/task-persistence/apiMessages.ts)
- [Goose 消息](https://github.com/block/goose/blob/main/crates/goose-provider-types/src/conversation/message.rs)、[会话数据库](https://github.com/block/goose/blob/main/crates/goose/src/session/session_manager.rs)、[导出命令](https://github.com/block/goose/blob/main/crates/goose-cli/src/commands/session.rs)
- [Continue 历史读写](https://github.com/continuedev/continue/blob/main/core/util/history.ts)、[会话/消息定义](https://github.com/continuedev/continue/blob/main/core/index.d.ts)
- Cursor：本机只读检查 globalStorage/state.vscdb 的表、JSON 字段和字段类型。所有新增测试值均为手写合成；非厂商承诺，也不覆盖加密/其他版本存储
- Grok Build：核对本机 1.0.46 随包 README 的 Session Persistence，明确 updates.jsonl 是权威恢复日志；结合本机原生记录和新运行逐字段验收，未把 chat_history / events 镜像当作第二份会话
- Cline CLI：核对本机 3.0.68 的原生 version=1 messages 快照及内部工具结果形状；测试值为合成，真实字段对照在内存中进行
- Hermes Agent：核对本机 hermes_state.py 的 schema、_encode_content / _decode_content 及原生 JSON/SQLite；结构化内容仅按客户端定义的 NUL+json 标记解码
- [Codex 分页记录保存策略](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/policy.rs)、[TurnItem 定义](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/items.rs)、[session_meta 的 history_mode](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)、[扩展项](https://github.com/openai/codex/blob/main/codex-rs/ext/items/src/lib.rs)

Kimi Code v2 的额外依据：[wire 类型](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/human/persist/v2/wire.ts)、[原生上下文折叠](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/human/persist/v2/fold.ts)、[消息定义](https://github.com/MoonshotAI/kimi-code/blob/main/packages/agent-core-v2/src/human/llm/message.ts)。


## 新增格式定义的来源

- WorkBuddy 5.3.14：随包 `cli/dist/codebuddy.js` 的持久化协议，以及本机权威会话 JSONL。未把同名模型或 CodeBuddy CLI 兼容性当作证据。
- Qoder 1.21.2 / ZCode 3.14.1 / Grok Bot 0.66.0：分别核对本机 JSONL、数据库 schema 及发行包的 native renderer / transcript 结构；真实数据仅在内存中对照，仓库测试均为合成。
- Cursor Agent 协议 2026.10.01-e373342：从官方 2026.10.01-e373342 发行包（仅下载源文件至临时目录，不安装、不运行入口）中 `proto/dist/generated/agent/v1/agent_pb.js` 提取 field number、scalar、oneof、enum 和消息引用，生成标准 FileDescriptorSet。`src/import/protocols/cursor-agent-2026.10.01.pb` 含 587 个消息类型；protobuf 事实不含运行时密钥或聊天。下载地址来自 [官方安装脚本](https://cursor.com/install)；独立对照直接使用厂商 SDK，未以本库输出生成预期。
- Zed：[DbThread 与 json/zstd 保存规则](https://github.com/zed-industries/zed/blob/main/crates/agent/src/db.rs)、[Message 外部标签](https://github.com/zed-industries/zed/blob/main/crates/agent/src/thread.rs)、[工具输入](https://github.com/zed-industries/zed/blob/main/crates/language_model_core/src/language_model_core.rs)、[工具结果内容](https://github.com/zed-industries/zed/blob/main/crates/language_model_core/src/request.rs)。这是所核对的源码契约，不等于本机发布版全能力验收。
- Warp：[固定版本 Task.proto](https://github.com/warpdotdev/warp-proto-apis/blob/00cdd6726f28fb6b32a6712f0ca3264dbd0956a1/apis/multi_agent/v1/task.proto)。最初按本机源码 Cargo.toml 的 78a78f21 核对，发现真实记录中有新字段后更新到上面固定 revision；协议集由 protoc 编译；文件为 `src/import/protocols/warp-00cdd672.pb`。临时生成时将 edition 2023 换成同字段号/类型的 proto3 描述并删除 Go-only feature 注释，以适配 prost-reflect；未改变 wire field number / 类型。Go-only 与 edition-only feature 注释不包含正文数据。此版本的 native Rust 持久化代码对 task 使用 prost encode/decode。
- Antigravity 2.19.1：实际调用本机原生语言服务，使用 LoadTrajectory / GetCascadeTrajectory 的完整返回值，逐项核对步骤结构和总数。[开源 RPC 客户端参考](https://github.com/mjacobs/agy-reader/blob/main/internal/daemon/client.go)；本机实测优先于参考项目的推断。提供的 macOS exporter 只连接回环服务、不发送新模型请求，去除内部请求头，0600 写入新文件；旧加密 pb、加密 db 的离线直接解密不在此 API 中。

```sh
python3 examples/export_antigravity.py CONVERSATION_ID /private/path/session.trajectory.json
cargo run --locked --example inspect_session -- antigravity /private/path/session.trajectory.json
cargo run --locked --example inspect_session -- cursor-cli /path/to/store.db AGENT_ID
cargo run --locked --example inspect_session -- zcode /path/to/db.sqlite SESSION_ID
cargo run --locked --example inspect_session -- zed /path/to/threads.db THREAD_ID
```

默认 max_file_bytes 是 200 MiB。本机 Warp 原始数据库约 211 MiB，验收显式使用
`ReadOptions { max_file_bytes: None, ..Default::default() }`；另用只含 agent 表的私有
临时副本核对计数。源码没有悄悄扩大默认限制。真实聊天副本未进入仓库。


## 本批全量真实记录与检查结果

2026-10-07 对当前发现的权威入口全量读取：WorkBuddy 29、Qoder 18、ZCode 50、
Grok Bot 18 个非空记录入口；Cursor Agent CLI 236 个非空 store 在默认限制内通过。
这些入口没有 unknown；Qoder 的工具 transcript 可能是主会话镜像，入口数不是
去重后的聊天数。另一个 Cursor store 触发默认 8 MiB 单 blob 限制，保持 TooLong 错误，
显式放宽调用方限制后也通过，输出 69 条消息、342 次调用和 342 份结果；本批
237 个 Cursor store 均已读取。目录中的调试日志不作为聊天记录。

WorkBuddy 全部 29 个入口的 345 条消息正文/角色、调用参数和工具原始结果与独立
JSONL 读取完全一致。Qoder 18 个入口的 115 条非空正文、215 次工具参数一致。
ZCode 50 个会话的 729 条消息、949 次调用及结果与独立 SQL 读取完全一致。
Grok Bot 18 个缓存 replica 的 1041 条消息正文/角色与独立读取一致。
Cursor 一个完整 store 的 127 条消息、1024 次调用及 1024 份结果与官方 SDK
逐字段一致，7 个外置 contentBlobId 的文本和来源与直接读 blob 一致，错误预期被拒绝。
Warp 五个会话 45 条消息、37 次调用、30 份结果与独立 protoc 生成的官方 Python SDK
逐字段一致；Antigravity 四个会话 30 条消息、54 次调用、54 份结果与原生服务轨迹
逐字段一致，并确认 149 步完整。以上对照不经过网络模型重新生成预期。

2026-10-07 检查时本机 Zed 数据库为空；2026-10-08 已补充下文的真实新线程验收。
此处 2026-10-07 的 Grok Bot 检查只覆盖
正文；本轮已补充原生工具轮廓与附件，完整参数/执行结果仍以原生是否保存为限。
Trae、Amp、Droid、Aider 等未取得原生证据的产品仍在
缺口表中，本次没有以名称或“通用 JSON”假装覆盖它们。


最终检查：Rust 1.88 `cargo test --all-targets --locked` **126 通过、0 失败、1 个隔离
进程辅助测试按设计 ignored**；Rust 1.95 文档测试 1 通过；clippy 全目标 `-D warnings`、
`fmt --check`、`git diff --check` 通过。Antigravity exporter 语法检查及原生完整导出通过。

## 可重复的真实客户端端到端测试

`tests/native_e2e.rs` 启动真实客户端，使用客户端当前的登录和模型配置，不使用 mock、
历史缓存或手写日志代替新会话。当前包含 9 个 CLI 入口：Codex、Grok Build、OpenCode、
Claude Code、Kimi Code、Cline CLI、Hermes、Cursor Agent CLI、WorkBuddy：

```sh
cargo test --locked --test native_e2e -- --ignored --nocapture --test-threads=1
```

也可以单独运行某个客户端，例如：

```sh
cargo test --locked --test native_e2e codex_native_e2e -- --ignored --nocapture
```

每次测试自动生成唯一标记和中文文件，预期在运行客户端前确定；prompt 仅告知输入文件名，
不提供答案。客户端必须通过工具读取文件、写出 `result.txt`，再回复完整内容。测试先核对
实际写出的文件，然后通过客户端返回的新会话 ID 选择原生持久化记录，由本库读取。
Codex/Grok/Claude/WorkBuddy 使用原生 JSONL；OpenCode 使用该新会话的官方 export；
Kimi 通过本次工作目录定位 v2 wire，Cline 使用隔离 data-dir 下的新 `.messages.json`；
Cursor CLI 通过新建 chat ID 定位 store，Hermes 通过唯一输入文件名定位原生数据库会话。

断言包括用户正文、最终助手正文、读取工具的参数、返回正文及 call ID 关联、
用户→调用→结果→回复顺序、每个事件的来源以及无 unknown。保存账单用量的格式还要求
非零输入/输出用量；Cursor CLI、Hermes、WorkBuddy 的该原生格式不保证保存账单用量，
不强制此项，也不补造用量。成功检查表明确记录 usage_checked 和 usage_present。
同一校验函数还必须拒绝错误预期。此任务不覆盖多模态、分支恢复、所有工具类型或
OpenCode SQLite 路径；这些仍须分别验收，不能由三次成功推断全部功能通过。

运行需要 PATH 中有对应客户端及有效登录，会实际调用当前配置的模型。
普通 `cargo test` 不发模型请求，这 9 个测试默认 ignored；只有上面的显式命令才运行。
客户端不存在、认证失败、180 秒超时、缺失原生日志或内容不一致均判失败，不跳过算成功。
测试打印每次运行的私有证据目录；Unix 目录权限为 0700、客户端输出文件为 0600。
目录保留 prompt、预期、实际结果、客户端版本、执行参数、stdout/stderr；任务成功后还保存
原生日志位置和检查表。执行参数不含环境变量或登录材料；失败不生成成功检查表，
不将真实会话或登录材料复制进仓库。无需用户手工出题或编辑测试样例。

2026-10-07 实际执行上面的三客户端命令：**2 通过、1 失败，命令退出码 101**。

| 客户端 | 本次自动端到端结果 | 证据目录名（完整路径由测试打印） |
|---|---|---|
| Codex 0.160.1 | 新会话完整任务、原生 JSONL 导入及全部上述断言通过；错误预期被拒绝 | `agent-sessions-e2e-codex-NrD0so` |
| OpenCode 1.18.27 | 新会话完整任务、官方原生 export 导入及全部上述断言通过；错误预期被拒绝 | `agent-sessions-e2e-opencode-eNaDSn` |
| Grok Build 1.0.46 | 失败；工具被本机 hook 拒绝，未写出结果，180 秒后终止本次进程组 | `agent-sessions-e2e-grok-KduqEh` |

Grok 的原始拒绝为 `Hook denied: VibeGuard: this integration accepts Bash hook events only`。
未修改本机 hook 或绕过其拒绝；此次不能计为 Grok 的自动端到端成功。
第一轮 OpenCode 未显式指定客户端 `--dir`，实际在仓库目录找测试文件而失败；
测试现已显式传入工作目录，重跑取得上表结果。两个失败均被实际文件断言发现，
没有因客户端返回成功退出码而误判通过。

增加测试后的普通检查也已实际完成：Rust 1.88 全目标 **126 通过、0 失败、4 ignored**
（原有隔离辅助测试 1 个，需显式运行的 live E2E 3 个）；Rust 1.95 文档测试 1 通过；
全目标 clippy `-D warnings`、fmt 和 diff 空白检查通过。普通检查通过不覆盖上表的失败。

2026-10-08 追加验收：重新运行 Grok 后，本次未再出现上述 hook 拒绝，但终端工具在
无交互模式下被取消，仍未生成结果文件。核对厂商随包 README 的
[权限规则](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-shell/README.md#permission-rules---allow---deny)
后，测试为本次调用显式添加 `--allow 'Bash(cat *)' --allow 'Bash(cp *)'`，不修改全局配置。

修正后实际运行 `cargo +1.88.0 test --locked --test native_e2e grok_native_e2e -- --ignored --nocapture`：
**1 通过、0 失败、0 ignored、2 filtered out，退出码 0**。Grok Build 1.0.46 完成文件任务，
新原生会话导入、正文、工具参数/结果关联、顺序、用量、来源及错误预期拒绝检查全部通过。
证据目录名为 `agent-sessions-e2e-grok-9iyxjL`，含 `checks.json` 和 `native-path.txt`。
本次针对该测试目标的 clippy `-D warnings`、fmt、diff 空白检查通过。

目前三条 live E2E 都已有成功运行证据：Codex/OpenCode 为上次结果，Grok 为本次补测；
本次没有重新运行完整三客户端命令，不将以上结果表述成新一轮三客户端全量通过。
其他 agent 和多模态、分支恢复等功能的验收缺口仍然存在。

同日发布候选 `0.3.0` 再次执行完整三客户端 live E2E 命令：**3 通过、0 失败、
0 ignored，退出码 0，耗时 52.23 秒**。三个新会话均完成文件任务、原生导入及错误预期拒绝。
证据目录名分别为 `agent-sessions-e2e-codex-Cu9POL`、
`agent-sessions-e2e-grok-GjBXnG`、`agent-sessions-e2e-opencode-7aCkAP`。
Rust 1.88 `cargo package --locked --allow-dirty` 打包并独立编译验证通过，
包含原生格式适配和两个 protobuf 描述文件；跨平台 CI 在提交后单独核查。

## 本轮内容适配验收（随后新增端到端的结果见下一节）

2026-10-08，`feat/complete-native-adapters` 工作树实际运行结果：

| 检查 | 结果与边界 |
|---|---|
| Rust 1.88 `cargo test --locked --all-targets` | **146 通过、0 失败、4 ignored**；20 项新增完整内容/回放回归，已有 50 项多客户端回归及 golden 等均通过；ignored 为 1 个隔离辅助测试及 3 个需显式启动的 live E2E |
| 显式运行完整 `native_e2e` | **3 通过、0 失败、0 ignored**，退出码 0，耗时 53.73 秒；Codex、Grok Build、OpenCode 均生成新会话，完成文件任务及预期反例校验 |
| Rust 1.88 文档测试 | 1 通过 |
| Rust 1.95 clippy、Rust 1.88 fmt、diff 空白检查 | 全目标 `-D warnings`、`fmt --check`、`git diff --check` 通过 |
| nightly 模糊测试 | `stream` 运行 60 秒，退出码 0，无崩溃；目标覆盖 26 个客户端快照入口和流式/投影边界；这不是全输入证明 |
| 真实入口批量读取 | WorkBuddy 29、Qoder 18、ZCode 50、Grok Bot 19 均非空、无 unknown；Cursor CLI 236 个默认限制内通过，另 1 个触发 8 MiB 限制，调用方显式取消体积限制后完整读取、无 unknown；共 237 个 store |
| 独立原生解码对照 | Cursor 官方 SDK：127 条消息、1024 次调用、1024 份结果一致，错误预期被拒绝；新增 326 个用户/思考 Content 载荷一致。Warp 官方 protobuf：45 条消息、37 次调用、30 份结果一致。Hermes 三个非空会话：272 个 reasoning/Codex 原生载荷与独立 SQL 读取一致 |
| Kimi 官方 fold 对照 | 独立运行官方上下文折叠函数的 8 组输入，正文、工具参数/结果、undo/clear/新旧压缩及 Unicode 预算投影一致；测试不涉及 todo store，按本库契约排除 SDK 临时生成的压缩省略提醒 |

本轮 live E2E 的私有证据目录位于系统临时目录，名称为
`agent-sessions-e2e-codex-ptYB81`、`agent-sessions-e2e-grok-MRyz2u`、
`agent-sessions-e2e-opencode-rXZOiB`，各含 `checks.json`、预先写入的预期、实际文件
和原生记录位置。仓库没有保存真实聊天或凭据。

复验本轮新增回归与真实客户端任务：

```sh
cargo +1.88.0 test --locked --test complete_content --test multi_agent
cargo +1.88.0 test --locked --all-targets
cargo +1.88.0 test --locked --test native_e2e -- --ignored --nocapture --test-threads=1
```

**仍未取得 26 个客户端全部真实新会话的验收证据。** Claude 的 403、Kimi 的 OAuth、
Cline CLI 的认证错误、Hermes 的 Unauthorized 是此前实际运行的阻塞；本轮没有改动
登录或模型配置。当时 Zed 没有原生线程，随后已补测；其他仅合成验收的来源仍以矩阵为准。不能由这次
普通测试和 3 个成功的 live E2E 推断所有版本、所有工具或未落盘字段都已支持。


## 端到端扩展与本次实际结果

2026-10-08，自动 live E2E 从 3 个入口扩展到 **9 个**，另用 Zed 内置 Agent
实际完成一个原生 GUI 任务。没有 mock 服务、预写对话或手工拼接厂商日志。
以下是逐项实际结果，不是 9 项全部通过：

| 客户端 | 本次结果 | 私有证据目录名 |
|---|---|---|
| Codex 0.160.1 | 通过：新会话、文件、原生 JSONL、全部任务断言及错误预期拒绝 | `agent-sessions-e2e-codex-EmQXSd` |
| Grok Build 1.0.46 | 通过：新会话、文件、原生 journal、全部任务断言及错误预期拒绝 | `agent-sessions-e2e-grok-h8q7xp` |
| OpenCode 1.18.27 | 通过：新会话、文件、官方 export、全部任务断言及错误预期拒绝 | `agent-sessions-e2e-opencode-7oo0ot` |
| Claude Code 2.1.281 | 失败：当前配置的服务返回 HTTP 403，订阅无访问权限 | `agent-sessions-e2e-claude-qBkgKg` |
| Kimi Code 0.29.0 | 失败：`auth.login_required`，`managed:kimi-code` 需要 OAuth 登录 | `agent-sessions-e2e-kimi-jqkUZL` |
| Cline CLI 3.0.68 | 失败：Unauthorized，客户端提示重新认证 | `agent-sessions-e2e-cline-PXrydM` |
| Hermes v0.17.0 | 失败：HTTP 401 unauthorized；客户端退出码虽为 0，缺少实际结果文件仍被验收拒绝 | `agent-sessions-e2e-hermes-E1jb9z` |
| Cursor Agent CLI 2026.09.18-9a7762b | 失败：新会话已创建，但 shell 被 hook 拒绝；180 秒超时 | `agent-sessions-e2e-cursor-agent-7tWxfo` |
| WorkBuddy 内附 CLI 2.115.0 | 失败：版本查询成功，任务运行 180 秒 stdout/stderr 均为空后超时；根因未确认 | `agent-sessions-e2e-workbuddy-QYNACW` |
| Zed 1.10.3 内置 Agent | 通过：GUI 新线程完成文件任务；修复解析问题后，原生数据库、正文、工具参数及结果关联、顺序、用量、来源和反例校验通过 | `agent-sessions-e2e-zed-3dt9kqbk` |

前三项以同一条筛选后的 Rust 测试命令重新执行，**3 通过、0 失败、6 filtered out**，
退出码 0，耗时 67.84 秒。新增六项分别执行，均失败；没有将客户端返回成功码、
测试默认 ignored 或超时视为通过。Kimi 前两次尝试使用了与 `--prompt` 冲突的权限参数，
已修正为官方 `--output-format stream-json --prompt` 并重跑；表中是修正后的 OAuth 错误。
Hermes 改用 `chat --toolsets terminal --max-turns 8 --query` 后取得表中的实际 HTTP 401。

Cursor 原始 hook 拒绝为 `VibeGuard: hook field cwd must be a nonempty string`，并明确
要求不要绕过被阻止的工具。本轮未改动 hook、全局配置、登录或模型设置。

Zed 通过原生 GUI 在独立临时项目中使用已有模型配置，授权仅限本次 `cat`/`cp` 调用。
输入文件和预期先于模型运行生成，prompt 不包含答案。实际写出的文件与预期逐字节一致。
新线程 ID 是 `52114283-8ff9-4ece-a788-f28f4b3b2944`，原生位置为
`~/Library/Application Support/Zed/threads/threads.db`。本库导入 2 条 Message、
1 次 ToolCall、1 份 ToolResult、1 项 Usage、2 项 Thinking Content，无 unknown。
UI 粘贴超时后再次输入导致同一个用户消息包含重复指令；两段都要求相同文件任务，
未把预期正文传给模型，验收确认原始指令存在于保存的用户消息中。

这次真实端到端首次暴露了 Zed 工具输入遗漏：原生 `ToolUse.input` 直接保存 JSON 对象，
旧实现只读 `{type,value}` 包装，错误地产生 `ToolArgs::Missing`。已按官方 SDK 的
反序列化规则修复：识别恰好两个字段的 json/text 包装，其他值原样保留为 JSON。
新增回归覆盖 raw JSON、tagged JSON/text、null 和带额外字段的对象。
修复后对同一真实线程重新导入，全部验收通过；同一校验函数拒绝错误答案，
也拒绝将工具参数故意改回 Missing 的结果。私有目录保留 `verify-zed.py`、
`checks.json`、导入结果、任务文件和原生记录位置；GUI 操作本身尚未加入 Rust 自动测试。

ZCode 也已打开并进入已登录任务界面，但原生目录选择框无法通过当前 UI 自动化确认；
尚未发送模型任务，不计为通过或客户端功能失败。准备目录为
`/tmp/agent-sessions-e2e-zcode-8ffy0gn6`，含未执行的任务和 `attempt.json`。
其余客户端仍按支持矩阵保留缺口。

本次代码修复后的检查：Rust 1.88 全目标 **147 通过、0 失败、10 ignored**
（1 个隔离辅助测试、9 个显式 live E2E），文档测试 1 通过；Rust 1.95 全目标
clippy `-D warnings` 通过；nightly 模糊测试 60 秒无崩溃，实际完成 125324 次输入执行。
普通测试不包含上述真实客户端的模型请求。

**当前有 4 个客户端取得真实新会话任务成功证据，其中 3 个可通过 Rust 自动重跑，
Zed 为本次实际 GUI 操作。仍不满足“26 个客户端全部端到端验收通过”。**
