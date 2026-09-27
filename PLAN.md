# agent-sessions 全生态交付计划

状态：执行中。用户已授权完成全套实现与验证。原始规划保存在 [v2 调研](docs/research/plan-v2.md)。
本计划以 [v0.1 实际实现](docs/specs/v0.1/PRODUCT.md) 和逐项源码核验为基础。

## 交付约定

- commit_policy: per_step。每个仓库独立提交；现有用户工作区保持原状。
- owner: 主 agent 负责集成；用户已授权子 agent 并行迁移，归属见 docs/delivery/THREADS.md。
- mode: execute_direct；verification_owner: 各仓库唯一执行者，主 agent 统一复核。
- 工作区：同级 `.agent-sessions-delivery/{repo}`；共享库在本目录。
- 依赖在发布前通过工作区外层 Cargo patch 验证，项目清单不写本机绝对路径。
- 原始记录归档必须保留原文和偏移；统计、标题、会话正文分开验证。
- 不修改真实记忆数据库，不上传真实会话或凭证；实机对比只读，导入 smoke 使用隔离数据。
- 发布前必须完成代码、版本、测试、打包、依赖可解析性和 CI 核验。
- 外部凭证/权限缺失时，完成所有独立工作，具体记录最后受阻的发布动作；不将其写成已完成。

## 执行步骤

| ID | 仓库/范围 | 完成条件与检查 | 状态/提交 |
|---|---|---|---|
| T0 | 本库：统一规划与契约 | 核对原始 v2、当前实现、所有消费者调用点；确定兼容边界 | 完成 a7d6250 |
| T1 | 本库：增量原始读取、history、标题索引、前缀消费 | 精确保真/偏移/截断、历史条目计数、索引来源、首个文本块测试；fmt/check/test/clippy/MSRV | 完成 bd21d7b |
| T2 | ccstats：weekly-reserve 上游化 | 补丁逐项核对，额度估算与 SDK 回归通过；Grok 补丁已有实现不重复加入 | 完成 d3de1f3 |
| T3 | ccstats：共享解析迁移 | Claude/Codex usage/tools/discovery 改用本库；保留标题索引契约；报表/缓存/冷启动基线对比，全量测试 | PR #190 已合并 8026edb；0.9.0 已发布，正式发布流程通过 |
| T4 | quotabar：正式 SDK 接入 | 所需 SDK 已发布或明确可验证候选；去 vendor 后 Rust/TS 测试与界面数据契约通过 | PR #188/#189 已合并；0.5.4 已公开发布，macOS App/DMG 签名与公证核验通过 |
| T5 | remem | 保留 raw 身份/完整性/过滤边界；spec 在先；focused/full tests、隔离导入 smoke、版本同步/preflight | PR #1091 已合并 91e3ee0，#1090 已关闭；PR/main CI 与 native 全绿；0.6.98 GitHub/crates.io/Homebrew/npm 已发布；MCP 注册完成，release 全绿 |
| T6 | refine | 公开 CLI 保持 Remem-only；迁移保留的 local library API；内部 legacy 分支未公开；workspace tests | PR #229 已合并 dce9e90；workspace 与会话测试、CI 通过 |
| T7 | ccp | 保留 64 KiB 前缀预算、首个文本块、100 字符预览、排序/UUID 规则；cargo test | PR #11 已合并 1c079a6；48 tests、真实摘要/usage 对比与 CI 通过 |
| T8 | chat-archive-rs | 仅抽取可共享的 raw framing/discovery；原文哈希、偏移恢复、未知记录/坏行保持原契约；cargo test | PR #29 已合并 283fac8；仓库无 CI，29 tests 与原文/偏移/哈希核验通过 |
| T9 | keepline / TS 文档 | Rust history 读取迁移，保留历史条目计数与错误显示；Rust/相关 TS checks；共享格式/fixture 文档 | PR #116/#117/#118/#119 已合并；桌面版 1.1.4 已公开发布，三平台构建及 macOS App/DMG 公证核验完成 |
| T10 | life-looper | ccstats CLI 版本/JSON/时区/范围/错误/超时/配置定价契约成立；Go tests；不再自行解析或硬编码降级价格 | PR #1 已合并 231caee；正式 ccstats CLI 联测通过，新 Looper 已安装，服务未启动 |
| T11 | 交付与发布 | crates/GitHub 发布链、消费方依赖 lockfile、PR/CI、迁移证据汇总；真实状态逐仓列明 | 8 个消费仓库迁移 PR 已全部合并；agent-sessions 0.2.0、ccstats 0.9.0、QuotaBar 0.5.4、Keepline 1.1.4 已发布；remem 0.6.98 已发布到 GitHub/crates.io/Homebrew/npm，MCP 注册完成；本批交付完成 |

## 已确定的设计选择

- 名称沿用 agent-sessions；Rust 源码共用，不新增 TS/Go 解析库。
- exec 与 Desktop 共现时保留 Exec 和原始客户端证据，不能据此声称无人发起。
- IDE 在库中独立标记；ccstats 既有 interactive scope 包含 IDE，避免新增不必要 CLI 开关。
- Codex 两套用量账本显式选择；同一次读取绝不同时计入。
- quotabar 的 vendor 有可复现来源；移除条件是全部依赖的 SDK 能力完成上游化并验证。
- ccp 前缀预算与严格快照边界分开；提前停止是正常消费策略。
- history 是历史条目，不能默认当作唯一会话数。
- 归档层保留原始字节，不能通过归一化 Event 反向重建原文。
- 标题索引、会话中的标题提示、首条提示词是不同来源；保持现有产品各自的读取契约。

证据和检查进度记录在 [WORKLOG](docs/delivery/WORKLOG.md)。
