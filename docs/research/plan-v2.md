# agent-sessions 完整规划 v2

> 一个 Rust 库：统一发现、流式解析 Claude Code / Codex（后续扩展更多 agent）的本地会话记录。
> 第一批用户是你自己的 6 个 Rust 项目（§1.4）；之后面向所有做 agent 用量统计、记忆、回放的 Rust 工具。
>
> 证据来源（2026-09-25）：ccstats `origin/main` 2e2a766、remem 7f4e144f、refine 88ceb45、quotabar origin/main 的源码；
> ccp、chat-archive-rs、keepline、stash 的 GitHub 默认分支；本机 `~/Desktop/code` 下全部 majiayu000 仓库的扫描；
> 本机 `~/.claude/projects`（134 个文件，98 MB，29,069 行）和 `~/.codex/sessions`
> （6,529 个文件，6.7 GB，按 1/8 抽样 817 个文件，212,452 行）的格式统计。
> 标注说明：**[事实]** 有代码或数据支撑；**[推断]** 需要进一步验证；**[决策]** 本规划的选择。
>
> v2 相对 v1 的变化：纳入 quotabar、ccp、chat-archive-rs、keepline、looper、stash 等仓库（§1.4、§5.4–5.8）；
> v0.2 增加 `history.jsonl` 读取；工期从 14 天调整为 18 天。

---

## 0. 一页结论

- **做什么**：`agent-sessions` crate，三层：`discover`（找文件）→ `read`（流式出 Event）→ 调用方各取所需。
- **为什么值得做**：不只是去重。你至少有 **7 个仓库**各自在找或解析这些文件（6 个 Rust、另有 TS/Go 若干），
  已经出现可量化的分歧和漏洞（§1.3）；Codex 9 月还出现了所有项目都不认识的新记录类型 `token_usage_record`。
- **v0.1 范围**：Claude Code + Codex 会话文件。v0.2 加 `history.jsonl`。Cursor、Gemini 放到 v0.3 之后。
- **迁移顺序**：ccstats → quotabar（跟着 ccstats 升级）→ remem → refine → ccp → chat-archive-rs → keepline 的 Tauri 部分。
- **非 Rust 项目**（keepline/Claude-Code-Monitor 的 TS 部分、stash、looper）不强行接入：共享格式文档和 fixture；looper 改为调用 `ccstats --json`。
- **工期估计**：约 18 个工作日（§6）。
- **开源增长路径**：会话格式参考文档 + 给 crates.io 上现有小工具提迁移 PR（§8）。

---

## 1. 现状

### 1.1 核心三个项目的代码分布 [事实]

| 项目 | 解析相关文件 | 相关测试数 | 需要的数据 |
|---|---|---|---|
| ccstats | `source/claude/parser.rs`(716)、`source/claude/tool_parser.rs`(203)、`source/codex/parser.rs`(701) | 76 | usage、model、message_id、stop_reason、endpoint、工具名 |
| remem | `memory/raw_transcript.rs`(372)、`ingest/sessions.rs`(发现部分)、`git_evidence.rs`(读 Codex `function_call` 参数) | 44 | 消息文本+时间戳、工具调用参数、流式+字节上限 |
| refine | `session/parser.rs`(771)、`session/discovery.rs`(367) | 63 | 消息文本、cwd、model、开始时间、来源模式、尾行截断标记 |

补充事实：
- refine 默认优先通过 remem CLI 读取会话（`ingest_sessions.rs` 的 `select_auto_provider`），找不到 remem 时才退回本地解析。所以 refine 的本地解析是 **独立运行时的备用路径**。
- ccstats 的 Cursor 数据来自 Cursor 官方用量 API（`cursor/client.rs`），remem 的 Cursor 数据来自 hook 生成的 JSONL（`session_rollup/cursor_transcript.rs`）。**两者数据源不同，不算重复。**
- remem 的 `session_rollup/parse.rs` 解析的是 LLM 输出的 XML，与会话文件无关。
- 三个项目都用 `serde_json 1` + `chrono 0.4`；ccstats 是 edition 2024，refine/remem 是 2021；MSRV 1.88。

### 1.2 真实数据格式调研 [事实]

**Claude Code**（版本全部是 2.1.x）
- 记录类型 19 种：`assistant` 11,007、`user` 5,978、`attachment` 4,483、`last-prompt`、`mode`、`permission-mode`、`ai-title`、`system`、`atis-latch`、`queue-operation`、`pr-link`、`file-history-snapshot/delta`、`custom-title`、`agent-name`、`cost-state`、`started`、`result`、`frame-link`。
- 内容块：`tool_use` 5,463、`tool_result` 5,461、`thinking` 3,666、`text` 1,872、`image`、`server_tool_use`。
- `isMeta` 只出现在 `user` 记录上（49 条）。
- `isSidechain: true` 的记录（5,727 条）**全部在 `subagents/` 目录下的文件里**，主会话文件里 0 条。
- 63/134 个文件是 subagent 文件。
- ccstats 还兼容旧的 `progress` 记录（`data.message` 内嵌 tool_use），本机当前数据已没有这种记录，但老用户可能还有。

**Codex**（cli_version 0.11x–0.156.1）
- 顶层类型：`response_item`、`event_msg`、`turn_context`、`session_meta`、`compacted`、`world_state`、`inter_agent_communication_metadata`，以及 **新出现的 `token_usage_record`**。
- `response_item` 子类型：`function_call`/`_output`、`reasoning`、`message`、`custom_tool_call`/`_output`、`web_search_call`、`tool_search_call`/`_output`、`image_generation_call`、`agent_message`。
- `event_msg` 子类型：`token_count`、`item_completed`、`task_started`、`task_complete`、`turn_aborted`、`thread_goal_updated`、`thread_settings_applied`。
- `token_usage_record`：9 月起出现（0.156.1 版本），每条带 `response_id` 和本次响应的 `usage`（含新字段 `cache_write_input_tokens`），与 `token_count` 一对一共存（最新 3 个文件计数完全相等）。**所有项目都没有处理它。**
- 单文件最大 185 MB；refine 的单文件上限是 200 MB，已经很接近。

**Codex 会话来源字段组合**（抽样 817 个会话）

| `payload.source` | `originator` | `thread_source` | 数量 | ccstats 判定 | refine 判定 |
|---|---|---|---|---|---|
| exec | codex_exec | – | 490 | Exec | Unattended |
| {subagent} | codex-tui | subagent | 132 | Subagent | Subagent |
| cli | codex-tui | user | 33 | Interactive | Interactive |
| exec | **Codex Desktop** | – | 32 | Exec | **Interactive** |
| {subagent} | **Codex Desktop** | – | 28 | Subagent | **Interactive** ❌ |
| **vscode** | Codex Desktop | – / user | 43 | **Unknown** ❌ | Interactive |
| {subagent} | codex-tui | – | 20 | Subagent | **Interactive** ❌ |
| 其他 | | | 39 | | |

### 1.3 已发现的问题 [事实，影响程度部分是推断]

| # | 问题 | 位置 | 影响 |
|---|---|---|---|
| P1 | refine 把 `source` 是 subagent 对象、但 `thread_source` 为空的会话判错 | refine `update_codex_session_mode` | 抽样中 188 个 subagent 会话里有 50 个（27%）被误判（49 个判成 Interactive，1 个判成 Unattended） |
| P2 | ccstats 不认识 `source: "vscode"`，判成 Unknown | ccstats `session_origin_from_source` | 抽样中 43 个（5%）IDE 会话不会出现在 `--scope interactive` 的结果里；quotabar 通过 ccstats 继承这个问题 |
| P3 | `exec` + `Codex Desktop` 组合，两边结论相反 | 同上 | 32 个（4%），需要定一个规则（§3.4） |
| P4 | 多个项目不读 `CLAUDE_CONFIG_DIR` / `CODEX_HOME`，写死 `~/.claude`、`~/.codex` | refine、remem（只缺 Claude）、chat-archive-rs、Claude-Code-Monitor（TS）、looper（Go） | 设置了自定义目录的用户会找不到会话 |
| P5 | refine 不过滤 `isMeta` | refine `parse_claude_code_line` | [推断] 用户消息数和 `is_substantial()` 判断偏高；本机只有 49 条，影响小 |
| P6 | 所有项目都不处理 `token_usage_record` | – | 当前与 `token_count` 冗余所以数字没错；[推断] 如果 Codex 以后去掉 `token_count`，所有用量统计会同时归零 |
| P7 | quotabar 以"源码压缩包 + 2 个补丁"的方式 vendor ccstats | quotabar `vendor/`、`scripts/prepare_sdk.mjs` | 基线停在 ccstats 845a5cb（09-06），落后 main 18 个提交；其中 Grok 4.7 定价补丁 [推断] 已被 main 的 2e2a766 取代，614 行的 weekly-reserve 补丁是否已合入上游未确认 |

### 1.4 其他仓库清单 [事实]

扫描范围：本机 `~/Desktop/code` 下 remote 为 majiayu000 的全部仓库，加上本机没有的 GitHub 仓库（keepline、stash、ccp）。

| 仓库 | 语言 | 做什么 | 与本库的关系 | 处理方式 |
|---|---|---|---|---|
| **quotabar**（53★） | Rust（Tauri）+ TS | 通过 ccstats SDK 取用量；自己不解析会话 | 间接用户 | 随 ccstats 升级；顺带改为依赖 crates.io 上的 ccstats，去掉 vendor（§5.4） |
| **ccp** | Rust | 读取 profile 目录下 `projects/**/*.jsonl` 的开头部分，拿首条用户消息作为会话摘要（`src/sessions.rs`，184 行） | 直接用户：自定义 Claude 根目录 + 只读开头 | 迁移（§5.5） |
| **chat-archive-rs** | Rust | 收集 Claude/Codex 会话和 `history.jsonl`（`src/collector.rs`，233 行，写死路径） | 直接用户，还需要 `history.jsonl` | v0.2 后迁移（§5.6） |
| **keepline** | TS + Rust（Tauri 菜单栏） | TS 部分有完整的 Claude 解析器；Tauri 的 `codex.rs`（610 行）读 Codex `history.jsonl` 做会话统计 | Tauri 的 Rust 部分可用 | v0.2 后迁移 Rust 部分（§5.7） |
| Claude-Code-Monitor（本机 claude-hub） | TS | 扫描 Codex `rollout-*.jsonl`，写死 `~/.codex` | 不能直接用 | 用格式文档和 fixture（§5.8） |
| stash（已归档） | TS | server 端有 Claude/Codex 解析器和 fixture | 不能直接用 | 不处理（已归档）；可以回收它的 fixture |
| looper | Go | `internal/costs.go`（691 行）自己解析会话并按硬编码价格算成本 | 不能直接用 | 改为调用 `ccstats --json`（§5.8） |
| rclean | Rust | 只用会话目录路径做"不要删除"的安全判断 | 可选使用 `Roots` | 低优先级 |
| harness、helixflow、specrail、vibeguard | Rust/Python | 只是透传或读取 `CLAUDE_CONFIG_DIR` / `CODEX_HOME` 找 skills 目录，不读会话 | 无关 | 不处理 |
| anywhere-ai、happy-cli | Python/TS | 2025 年的旧项目 / fork | 无关 | 不处理 |

注：GitHub 代码搜索只覆盖部分仓库和默认分支，本机没有的仓库可能还有遗漏（[推断] 例如 cc-model-watch 未搜到相关代码）。

---

## 2. 目标、非目标、成功标准

### 目标
1. 6 个 Rust 仓库的 Claude/Codex 会话发现和解析改用 `agent-sessions`（quotabar 通过 ccstats 间接使用），删掉各自的旧实现。
2. 修掉 §1.3 的 P1–P5、P7，并支持 P6。
3. 格式变化能被第一时间发现（未知记录类型计数 + fixture 回归）。
4. 作为独立开源库发布到 crates.io；格式文档同时服务你的 TS/Go 项目和外部用户。

### 非目标（留在各项目）
- 定价、成本（ccstats）；缓存（ccstats `source/cache.rs`）；跨文件去重的状态（ccstats `loader.rs`）。
- 增量游标、SQLite、raw_messages 去重（remem）；脱敏（remem）。
- 切块、过滤、聚合、LLM 提取（refine）。
- 实时监听文件（watch）：v0.x 不做，调用方自己轮询。
- TS / Go 版本的库：不做。非 Rust 项目共享格式文档和 fixture 即可。

### 成功标准
| 指标 | 目标 |
|---|---|
| ccstats 迁移前后 `daily/monthly/session/tools --json` 输出 | 除 P2 修复引起的差异外完全一致 |
| quotabar | 不再 vendor ccstats，改依赖 crates.io 版本；界面数字与迁移前一致（P2 差异除外） |
| remem 对本机全部会话文件解析出的消息 | 迁移前后逐条一致 |
| refine 本地路径解析结果 | 除 P1/P5 修复引起的差异外一致 |
| ccp、chat-archive-rs、keepline | 各自测试通过；会话列表与迁移前一致 |
| 性能 | ccstats 冷启动全量解析耗时不超过迁移前的 105% |
| 删除代码 | 全部项目合计净删除 ≥ 2,000 行（[推断] 根据 §1.1、§1.4 行数估算） |
| 外部采用（发布后 3 个月） | crates.io 下载 ≥ 1,000/月，至少 1 个非你本人的依赖方 |

---

## 3. 设计

### 3.1 分层

```
Roots::from_env()          读 CLAUDE_CONFIG_DIR / CODEX_HOME，否则用 ~/.claude、~/.codex
      │                    （ccp 这种多 profile 工具直接构造 Roots { claude: Some(profile_dir), .. }）
discover(&roots, &filter)  → Discovery { files: Vec<SessionFile>, errors }
      │                        SessionFile { agent, path, kind: Main|Subagent, size, modified }
read(&file, &opts)         → SessionReader: Iterator<Item = Result<Event, LineError>>
      │                        .finish() → ReadSummary { truncated_tail, unknown_types, bytes_read, lines }
调用方                      ccstats 取 Usage/ToolCall；remem 取 Message/ToolCall；refine 取 Message/Meta；
                           ccp 只读开头取首条 Message；chat-archive-rs 取全部 Message（v0.2 加 history）
```

### 3.2 公开 API（v0.1）

```rust
// ---- 发现 ----
pub enum Agent { ClaudeCode, Codex }                    // #[non_exhaustive]
pub enum FileKind { Main, Subagent }
pub struct Roots { pub claude: Option<PathBuf>, pub codex: Option<PathBuf> }
impl Roots { pub fn from_env() -> Self; pub fn from_home(home: &Path) -> Self; }

pub struct DiscoverFilter {
    pub agents: Vec<Agent>,                  // 空 = 全部
    pub include_subagents: bool,             // 默认 false
    pub modified_after: Option<SystemTime>,
}
pub struct SessionFile { pub agent: Agent, pub path: PathBuf, pub kind: FileKind, pub size: u64, pub modified: SystemTime }
pub struct Discovery { pub files: Vec<SessionFile>, pub errors: Vec<DiscoverError> }
pub fn discover(roots: &Roots, filter: &DiscoverFilter) -> Discovery;
pub fn classify(path: &Path, roots: &Roots) -> Option<SessionFile>;   // hook 场景：已知路径

// ---- 读取 ----
pub struct ReadOptions {
    pub max_file_bytes: Option<u64>,         // 超过 → ReadError::TooLarge
    pub max_line_bytes: Option<usize>,       // 单行超过 → LineError::TooLong，继续读下一行
    pub stop_at_byte: Option<u64>,           // 只读前 N 字节（remem 的 byte_limit、ccp 的读开头）
    pub include: EventKinds,                 // 位掩码：只解析需要的事件，省 CPU
}
pub fn read(file: &SessionFile, opts: &ReadOptions) -> Result<SessionReader, ReadError>;
pub fn read_from<R: BufRead>(agent: Agent, reader: R, opts: &ReadOptions) -> SessionReader<R>; // 测试/内存数据

pub enum Event {                                         // #[non_exhaustive]
    Meta(MetaUpdate),
    Message(Message),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Usage(Usage),
}
pub struct Located<T> { pub line_no: u64, pub at: Option<DateTime<Utc>>, pub value: T }

pub struct MetaUpdate {                                  // 一个会话内多次出现，由 SessionMeta::apply 合并
    pub session_id: Option<String>, pub cwd: Option<String>, pub git_branch: Option<String>,
    pub model: Option<String>, pub origin: Option<Origin>, pub agent_version: Option<String>,
    pub title: Option<String>,                           // Claude 的 ai-title / custom-title
}
pub enum Origin { Interactive, Ide, Exec, Subagent, Unknown }
pub struct Message { pub role: Role, pub text: String, pub is_meta: bool, pub is_sidechain: bool }
pub enum Role { User, Assistant, System, Developer }
pub struct ToolCall { pub id: Option<String>, pub name: String, pub arguments: ToolArgs }
pub enum ToolArgs { Json(serde_json::Value), RawString(String) }   // Codex 的 arguments 是字符串化 JSON，原样保留
pub struct ToolResult { pub call_id: Option<String>, pub is_error: Option<bool>, pub text: String }
pub struct Usage {
    pub dedup_key: Option<String>,           // Claude: "claude:" + message.id；Codex: response_id 或 "文件:序号"
    pub model: Option<String>,
    pub input: u64, pub output: u64, pub cache_read: u64,
    pub cache_write: u64, pub cache_write_1h: u64, pub reasoning: u64,
    pub reported_total: Option<u64>,
    pub stop_reason: Option<String>,
    pub endpoint: Endpoint,                  // Claude inference_geo → Native / Proxy / Unknown
}

pub struct SessionMeta { /* 合并后的 MetaUpdate + started_at */ }
impl SessionMeta { pub fn apply(&mut self, update: &Located<MetaUpdate>); }

pub struct ReadSummary { pub lines: u64, pub bytes_read: u64, pub truncated_tail: bool, pub unknown_types: BTreeMap<String, u64> }
```

**v0.2 新增**（chat-archive-rs、keepline 需要）：
```rust
pub fn history_files(roots: &Roots) -> Vec<(Agent, PathBuf)>;        // ~/.claude/history.jsonl、~/.codex/history.jsonl
pub fn read_history(agent: Agent, path: &Path, opts: &ReadOptions) -> HistoryReader;  // Item = Result<HistoryEntry, LineError>
pub struct HistoryEntry { pub session_id: Option<String>, pub text: String, pub at: Option<DateTime<Utc>>, pub project: Option<String> }
```
（`history.jsonl` 的具体字段需要在 v0.2 开始前用 survey 确认。）

### 3.3 错误契约 [决策]

| 情况 | 行为 |
|---|---|
| 目录不存在 | `Discovery.files` 为空，不报错（没装这个 agent 是正常情况） |
| 目录无权限或 I/O 失败 | 写入 `Discovery.errors`，其他目录继续扫描 |
| 文件打不开 / 超过 `max_file_bytes` | `read()` 返回 `ReadError` |
| 中间某行不是合法 JSON | 迭代器产出 `LineError::InvalidJson { line_no }`，**由调用方决定**跳过还是整个文件失败 |
| 最后一行没有换行且 JSON 不完整 | 不报错，`truncated_tail = true`（"文件是否还在写入"的时间窗口判断由调用方做，例如 remem 的 60 秒规则） |
| JSON 合法但不认识的 `type` | 不产出事件，计入 `unknown_types` |
| 认识的类型但缺字段 | 缺的字段为 `None`，不编造默认值；必需字段缺失（如 Usage 没有任何 token 数）则不产出该事件 |
| 永不 panic | fuzz 测试保证 |

说明：refine 现在遇到中间坏行就整个文件失败，remem 是记录失败但不推进游标，keepline 的 Codex 统计会把坏行信息展示出来。这些做法都可以在这个接口上原样实现，**库不替调用方做降级决定**。

### 3.4 统一规则 [决策]

| 问题 | 规则 | 依据 |
|---|---|---|
| Codex 来源（P1–P3） | ① `thread_source == "subagent"` 或 `source` 是含 `subagent` 键的对象 → Subagent；② `source` 为 `exec` → Exec；`cli` → Interactive；`vscode` → Ide；③ 以上都没有时才看 `originator`（codex_exec → Exec；codex-tui / codex_cli_rs → Interactive；Codex Desktop → Ide）；④ 会话内多次观察取优先级最高的（Subagent > Exec > Ide > Interactive > Unknown） | `source` 是 Codex 写的结构化字段，`originator` 是客户端名字，前者更可靠。`exec + Codex Desktop` 按 `source` 判为 Exec（[推断] Desktop 内部可能用 exec 跑自动化任务，**需要你确认**） |
| 新增 `Ide` 类型 | refine 可以映射回 Interactive，ccstats 可以增加 `--scope ide` 或并入 interactive | 保留信息，让调用方决定 |
| Codex 用量来源（P6） | 优先用 `token_usage_record`（有 response_id，天然可去重）；文件里没有这种记录时退回 `token_count` 累计差分（沿用 ccstats 的 `subtract` / `is_duplicate_of`）；同一文件两种都有时只用前者 | 两者一一对应 |
| Codex model | 跟踪 `turn_context.payload.model`，Usage 取当时的 model | 沿用 ccstats |
| Claude 用量去重键 | `message.id`（同一消息被拆成多行、每行重复 usage） | 沿用 ccstats |
| isMeta（P5） | 库打标签不过滤；remem、refine 在调用方过滤 | 过滤是用途相关的决定 |
| subagent | 发现时打 `FileKind::Subagent`，消息打 `is_sidechain` | ccstats 要计费，其他项目不要；ccp 的 subagent 文件不可 resume，本来就要排除 |
| Claude `progress` 旧格式 | 继续支持内嵌 tool_use | 老用户的历史数据 |
| 文本提取 | Claude：`text` 块；user 的字符串 content；不含 `thinking`、`tool_result`（后者单独成 ToolResult）。Codex：`message` 的 `input_text` / `output_text` / `text` 块，`developer` 角色单独标出 | 合并 refine 与 remem 现有规则 |

### 3.5 性能设计
- 逐行读取（`BufRead::read_until`），不一次性读入整个文件；内存只和最长的一行相关。
- 先用借用反序列化读出 `type` 等少数字段（ccstats 现在就是 `RawJsonEntry<'a>`），只对需要的事件再完整解析。
- `ReadOptions.include` 让 ccstats 跳过消息文本提取，让 remem 跳过用量计算；`stop_at_byte` 让 ccp 只读开头。
- 基准：`benches/` 用 criterion 对 fixture 语料测吞吐；`examples/scan.rs` 对本机全量数据测冷启动耗时。
- 参考数字：ccstats 0.2.62 在本机 `codex monthly --json --offline` 耗时 5.05 秒（有缓存，不代表冷启动）。迁移前需要先测一次冷启动基准。
- quotabar 是常驻菜单栏应用，并且已经有手动释放内存的代码（`relieve_allocator_pressure`），说明内存敏感；库的流式设计对它有利。

### 3.6 仓库结构（每个文件 ≤ 200 行）

```
agent-sessions/
├── Cargo.toml                 edition 2024, rust-version 1.88, deps: serde, serde_json, chrono
├── src/
│   ├── lib.rs                 re-export
│   ├── agent.rs               Agent, FileKind, Origin, Role
│   ├── event.rs               Event 及各载荷类型
│   ├── error.rs               ReadError, LineError, DiscoverError
│   ├── roots.rs               Roots::from_env / from_home
│   ├── discover.rs            目录遍历、subagent 判定、过滤
│   ├── reader.rs              逐行读取、字节上限、尾行判定、unknown_types 计数
│   ├── meta.rs                SessionMeta 合并规则
│   ├── history.rs             （v0.2）history.jsonl
│   ├── claude/{mod,record,message,usage,tool}.rs
│   └── codex/{mod,record,message,usage,origin,tool}.rs
├── tests/
│   ├── fixtures/{claude,codex}/<版本>/<场景>.jsonl   + 同名 .expected.json
│   ├── golden.rs
│   └── contract.rs
├── fuzz/                      cargo-fuzz：单行解析
├── benches/parse.rs
├── examples/{scan,survey}.rs  survey = 输出本机记录类型分布，用于发现新格式
├── scripts/redact.py          把真实会话脱敏成 fixture
└── docs/formats/{claude-code,codex,history}.md   格式参考文档（也给 TS/Go 项目用）
```

---

## 4. 测试与格式跟踪

### 4.1 fixture 语料
1. **收集现有的**：ccstats `tests/fixtures/claude/session.jsonl`、remem `tests/fixtures/codex-rollout-minimal.jsonl`、stash `server/src/adapters/claude/fixtures/`（含一个坏文件样本），以及各项目测试里内联的 JSONL 片段（核心三个项目共 183 个相关测试）。
2. **从本机数据挑**：按 agent 版本 × 场景各挑一个。场景包括普通对话、工具调用、subagent、上下文压缩（`compacted`）、中断（`turn_aborted`）、写入中的尾行、`token_usage_record` 新格式、旧 `progress` 格式、Codex 来源表（§1.2）里的每一种组合。
3. **脱敏**：`scripts/redact.py` 保留结构、键名、类型和数值；把所有字符串值替换成等长占位文本；路径统一改成 `/home/user/project`。提交前人工抽查，并用 vibeguard/gitleaks 扫描。**这一步不能跳过，会话里有代码和密钥。**

### 4.2 测试层次
| 层 | 内容 |
|---|---|
| 单元 | 每条统一规则（§3.4）一个测试，特别是 Codex 来源判定表里的每一行组合 |
| golden | 每个 fixture 的完整 Event 输出和 expected 文件逐字段比较；更新用 `UPDATE_GOLDEN=1` |
| 契约 | §3.3 每一行一个测试 |
| fuzz | 单行解析 + 尾行判定，CI 每次跑 60 秒，每周跑 1 小时 |
| 对比（迁移期） | 每个消费方迁移 PR 里加一个临时测试：旧解析器 vs 新库，对本机全部文件逐条对比；迁移完成后删掉 |

### 4.3 格式跟踪（长期维护的核心）
- `examples/survey.rs`：扫描本机数据，输出记录类型、agent 版本分布，与 `docs/formats/*.md` 里的已知类型对比，列出新类型。
- **每周运行一次**（在你本机用定时任务，CI 上没有真实会话数据），发现新类型就：脱敏样本 → 加 fixture → 决定是否解析 → 发 patch 版本 → 更新格式文档（TS/Go 项目看文档跟进）。
- 调用方看到 `ReadSummary.unknown_types` 非空时打 warn 日志，ccstats、remem、quotabar 的用户也能帮你发现格式变化。

---

## 5. 各项目迁移清单

### 5.1 ccstats（第一个迁移，最能检验正确性）
| 替换 | 保留 |
|---|---|
| `source/claude/parser.rs` 的 JSONL 解析 → 库的 `Usage` | `normalize_model_name`（定价相关） |
| `source/claude/tool_parser.rs` → 库的 `ToolCall` | `RawEntry`、`ToolCall` 等内部类型，新增薄转换文件 `source/agent_sessions_adapter.rs` |
| `source/codex/parser.rs` 的逐行处理、累计差分、来源判定 → 库 | `codex/quota.rs`、scope 过滤、`loader.rs` 去重、`cache.rs` |
| `find_claude_files` / `find_codex_files` → `discover` | `CodexScope` 增加 `Ide`（或并入 Interactive，由你决定） |
| `source/session_titles.rs` 里读 Claude 标题的部分 → `MetaUpdate.title` | SDK 对外的 `load_session_titles` 接口不变 |

验证：
```
# 迁移前（main）和迁移后各跑一次，结果写到文件后 diff
ccstats daily --json --offline; ccstats session --json --offline; ccstats tools --json --offline
ccstats codex monthly --json --offline; ccstats codex daily --json --offline
cargo test && cargo clippy --all-targets -- -D warnings
```
diff 应只包含 P2（vscode 会话）引起的 scope 差异。ccstats 的缓存 key 要带上 `agent_sessions` 的版本号，否则旧缓存会掩盖差异。
**ccstats 的 SDK 公开接口不变**，这样 quotabar 升级时只需要改版本号。

### 5.2 remem
| 替换 | 保留 |
|---|---|
| `memory/raw_transcript.rs` 的 `parse_transcript_message`、`parse_codex_response_item`、`extract_content_text`、`transcript_timestamp_epoch`、`stream_reader`/`visit_line` | `classify_transcript_line` 的时间窗口逻辑、isMeta 过滤策略（改用 `is_meta` 标签实现） |
| `git_evidence.rs` 里 `from_codex_transcript` 对 `function_call` 的解析 → 库的 `ToolCall` | commit 命令识别逻辑 |
| `ingest/sessions.rs` 的 `default_scan_roots`、`collect_jsonl_files` → `Roots::from_env` + `discover` | `ingest_cursors` 游标、60 秒活跃尾行窗口、`raw_messages` 去重 |
| – | Cursor 相关（`cursor_transcript.rs`、`cursor_hook`）暂不动 |

验证：临时对比测试（本机全部 Claude + Codex 文件的 (role, text, 时间戳) 序列）；`cargo test`；在复制出来的数据库上跑一次 `remem ingest-sessions`，确认新增行数为 0（幂等）。

### 5.3 refine
| 替换 | 保留 |
|---|---|
| `session/parser.rs` 的 Claude/Codex 解析、`update_codex_session_mode` → 库 | `Session`、`SessionMeta` 类型（改由库的事件填充）；`MAX_SESSION_FILE_BYTES` 改为传给 `ReadOptions` |
| `session/discovery.rs` → `discover` | remem provider 优先的选择逻辑 |
| – | 切块、过滤、聚合、remem archive 读取 |

行为变化（在 PR 里写明）：P1 修复后部分会话从 Interactive 变成 Subagent；P5 修复后过滤 `isMeta`；P4 修复后支持自定义目录。
验证：`cargo test --workspace`；临时对比测试列出所有差异，逐条确认属于 P1/P4/P5。

### 5.4 quotabar（跟随 ccstats）
- 前提：ccstats 完成迁移并发布新版本到 crates.io（当前 crates.io 最新是 0.8.0）。
- 步骤：
  1. 先确认 `ccstats-weekly-reserve.patch`（614 行）的内容是否已经进入 ccstats main；没有的话**先把它合进 ccstats 上游**。
  2. 确认 Grok 4.7 定价补丁已被 ccstats 2e2a766 取代。
  3. `src-tauri/Cargo.toml` 从 `path = "../vendor/ccstats"` 改为 crates.io 版本依赖；删除 `vendor/ccstats-sdk.*`、两个补丁、`scripts/prepare_sdk.mjs` 里对应的步骤。
- 收益：修掉 P7；quotabar 自动获得 P2/P6 的修复。
- 验证：`cargo test`（src-tauri）、`bun test`；在菜单栏里对比迁移前后今天、本周、本月的成本数字。
- 本地 quotabar 落后 main 201 个提交，开始前先同步。

### 5.5 ccp
- 替换 `src/sessions.rs` 的目录扫描和"读开头找首条用户消息"逻辑 → `discover`（用 `Roots { claude: Some(profile_home) }`）+ `read` 配合 `stop_at_byte` 和 `include = Message | Meta`。
- 保留：`is_resumable_id`（UUID 文件名判断）、按修改时间排序和 limit。
- 收益：会话标题可以直接用 `MetaUpdate.title`（Claude 的 ai-title），比首条消息更准确（[推断] 需要确认 ai-title 出现在文件开头附近）。
- 验证：`cargo test`；对你的每个 profile 跑一次会话列表，和迁移前对比。

### 5.6 chat-archive-rs（v0.2 发布后）
- 替换 `src/collector.rs` 的 `discover_sources`、`walk_jsonl`、`stream_records_from_source` → `discover` + `read` + `read_history`。
- 修掉 P4（目前写死 home 路径）。
- 本地仓库最后更新是 04-11，GitHub 上有 09-12 的推送，开始前先同步。

### 5.7 keepline 的 Tauri 部分（v0.2 发布后）
- `menubar-tauri/src-tauri/src/codex.rs` 的 `history.jsonl` 统计 → `read_history`。账号、JWT、限额相关代码不动。
- TS 部分（`src/adapters/claude/parser/`）不迁移，按 §5.8 处理。

### 5.8 非 Rust 项目
| 项目 | 处理 |
|---|---|
| looper（Go） | `internal/costs.go` 自己解析会话并用硬编码价格算成本（691 行），和 ccstats 功能重复，且价格会过时。改为执行 `ccstats --json --since <date>` 读结果。**这是最省事的一步，不依赖本库，可以随时做** |
| keepline / Claude-Code-Monitor 的 TS 解析器 | 不做 TS 版本的库。以 `docs/formats/*.md` 为准，格式变化时对照更新；可以把 fixture 目录复制过去做测试 |
| stash | 已归档，不处理；回收它的 fixture |

---

## 6. 里程碑与工期（估计，按每天 4–6 小时专注时间）

| 阶段 | 内容 | 预计 | 完成标准 |
|---|---|---|---|
| M0 | 建仓库、CI（fmt、clippy、test、MSRV 1.88）、脱敏脚本、收集 fixture | 2 天 | ≥ 20 个 fixture，CI 通过 |
| M1 | discover + reader + Claude 解析 | 2 天 | golden、契约测试通过 |
| M2 | Codex 解析（含来源规则、token_usage_record、差分）、fuzz、bench | 3 天 | 来源判定表每一行都有测试；fuzz 60 秒无 panic |
| M3 | 发布 v0.1.0；格式参考文档 | 0.5 天 | docs.rs 可访问 |
| M4 | ccstats 迁移并发布新版本 | 2 天 | §5.1 diff 只含预期差异，性能 ≤ 105% |
| M5 | quotabar 去掉 vendor，改用 crates.io 上的 ccstats | 1 天 | §5.4 验证通过 |
| M6 | remem 迁移 | 2 天 | §5.2 对比零差异，重复 ingest 新增 0 行 |
| M7 | refine 迁移 | 1.5 天 | §5.3 差异全部可解释 |
| M8 | ccp 迁移 | 0.5 天 | §5.5 验证通过 |
| M9 | v0.2：`history.jsonl`；根据前面迁移发现的 API 问题调整 | 1.5 天 | survey 确认 history 字段；测试通过 |
| M10 | chat-archive-rs、keepline Tauri 部分迁移 | 1.5 天 | §5.6、§5.7 验证通过 |
| M11 | README、示例、格式文档定稿；所有项目改为依赖 crates.io 正式版本 | 0.5 天 | 没有项目再用 path/git 依赖 |

合计约 18 天。另外 looper 改用 `ccstats --json`（§5.8）与本库无关，可以随时穿插做，约 0.5 天。

### 之后
- **v0.3**：Cursor（remem 的 hook 转写格式）、Gemini CLI。先用 `survey` 看本机有没有数据再决定。
- **v0.4+**：ccstats 另外约 16 个来源，按有没有外部用户需要逐个评估，不主动全部迁移。

---

## 7. 版本与发布

- 0.x 期间：minor 版本可以有破坏性变更（CHANGELOG 写明迁移方法）；patch 版本只加新格式支持、修 bug。
- 公开的 enum 都加 `#[non_exhaustive]`，新增 agent 或事件类型不算破坏性变更。
- 发布流程沿用 loom、ccstats 的方式：打 tag `vX.Y.Z` 后由 CI 发布到 crates.io（需要 `CARGO_REGISTRY_TOKEN`）。
- 依赖链：`agent-sessions` → `ccstats` → `quotabar`。agent-sessions 发 patch 后，ccstats 需要跟着发版，quotabar 才能拿到修复。在 ccstats 的 CI 里加 Dependabot/Renovate 自动提升级 PR。
- 1.0 的条件：所有迁移项目稳定使用 3 个月以上，且至少有 1 个外部依赖方。

---

## 8. 开源推广

依据：之前的 GitHub 数据显示，你的项目里能长期稳定增长的是被搜索引擎收录的内容（claude-skill-registry）和被别人依赖的库（jsonrepair-rs）；靠发帖带来的 star 两周左右就停了。

1. **格式参考文档**：`docs/formats/claude-code.md`、`codex.md`、`history.md`，写清每种记录类型、字段、出现的版本。[推断] 网上目前没有系统的公开文档，发布前需要搜索确认。
2. **给现有小工具提迁移 PR**：crates.io 上的 codexusage、codex-recall、agent-recall、clawgs 都在自己解析这些格式（下载量都在几十到几百）。被合并就是外部依赖方。
3. README 第一屏写"被 ccstats / quotabar / remem / refine / ccp 使用"，并附 §1.2 的来源判定对比表。
4. 发布 v0.1 时发一条 X 帖子，用 §1.3 的具体数字（27% 的 subagent 会话被误判），不做空泛宣传。

---

## 9. 风险

| 风险 | 可能性 | 应对 |
|---|---|---|
| Codex / Claude Code 格式频繁变化 | 高（本机已见 0.11x→0.156 期间多次新增类型） | 这正是库存在的理由；每周 survey + unknown_types 告警 |
| 脱敏不彻底，泄露代码或密钥 | 中 | 脚本替换所有字符串值 + 人工抽查 + vibeguard/gitleaks 扫描 fixture 目录 |
| ccstats 迁移后变慢 | 中 | 借用反序列化 + `include` 位掩码；先测冷启动基准，超过 105% 不合并 |
| 依赖链变长（agent-sessions → ccstats → quotabar），修复传递慢 | 中 | ccstats 用自动升级 PR；quotabar 不再 vendor |
| 抽象不合适，各项目需求差异太大 | 中 | 库只做"文件 → 事件"，所有策略判断（过滤、去重状态、降级）留给调用方 |
| weekly-reserve 补丁没有合入 ccstats 上游 | 中 | M5 开始前先确认；未合入就先合进 ccstats |
| exec + Codex Desktop 的判定规则错误 | 低 | 需要你确认；规则集中在 `codex/origin.rs`，改一处即可 |
| 维护负担增加 | 中 | 净删除 ≥ 2,000 行；格式变化以后只改一处，不再是 6 处 |

---

## 10. 需要你拍板的事

1. **crate 名**：`agent-sessions`（crates.io 可用）。
2. **exec + Codex Desktop 判为 Exec 还是 Interactive？** 你在 Desktop 里是否有自动化或定时任务？
3. **ccstats 的 `Ide` 来源**：新增 `--scope ide`，还是并入 interactive？
4. **仓库**：是否在 GitHub 上用 `majiayu000/agent-sessions` 公开建仓？
5. **quotabar 的 weekly-reserve 补丁**：它是 quotabar 专用的逻辑，还是本来就应该进 ccstats？
6. **范围**：chat-archive-rs 和 keepline 还在维护吗？如果不维护，M9–M10 可以砍掉，工期降到约 15 天。
