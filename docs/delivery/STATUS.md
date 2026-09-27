# 交付状态

## 已发布

- agent-sessions 0.2.0 已发布到 crates.io 和 GitHub Release。
- 发布源码 3ae0828；Ubuntu/MSRV、macOS、Windows、lint、fuzz CI 全部通过。
- 发布包 checksum：741368addca6a3758a911dc5b0871df029863c67d2d2008788662586ed4748c3。

## 消费方

| 仓库 | 可审阅结果 | 已完成的主要验证 | 剩余门槛 |
|---|---|---|---|
| ccstats | [PR190](https://github.com/majiayu000/ccstats/pull/190)，已合并 8026edb；0.9.0 已发布，release workflow 36211757898 全部 14 项通过 | 1002完整测试、审查修复后11回归；桌面15 Rust/36 Web/1 IPC；最新远端5checks全绿；独立review闭环 | 完成；本机 Cargo 安装 0.9.0，Looper 联测通过 |
| refine | [PR229](https://github.com/majiayu000/refine/pull/229)，已合并 dce9e90 | 654workspace、108会话测试；真实数据对比；最新10checks全绿 | 无；记录 main CI 结果 |
| ccp | [PR11](https://github.com/majiayu000/ccp/pull/11)，已合并 1c079a6 | 48tests、真实摘要和usage对比；CI通过 | 无；记录 main CI 结果 |
| chat-archive-rs | [PR29](https://github.com/majiayu000/chat-archive-rs/pull/29)，已合并 283fac8 | 29tests、真实原文/offset/hash/ID/恢复位置零差异、registry locked check | 已完成合并；仓库无CI |
| life-looper | [PR1](https://github.com/majiayu000/life-looper/pull/1)，已合并 231caee；候选发布版CLI已联测 | Go premerge、3Node渲染回归、真实CLI的范围/价格/告警联测 | 完成；实际 CLI 联测通过，新 Looper 已安装，服务保持未启动 |
| quotabar | PR188/PR189 已合并；[v0.5.4](https://github.com/majiayu000/quotabar/releases/tag/v0.5.4) 已公开发布 | 四平台正式构建通过；五个安装包及校验文件哈希一致；两种 macOS DMG 与 App 的签名、公证票据、Gatekeeper 核验通过 | 完成 |
| keepline | PR116/117/118/119 已合并；[v1.1.4](https://github.com/majiayu000/keepline/releases/tag/v1.1.4) 已公开发布 | 修复 npm/Tauri 依赖漂移和 DMG 公证；三平台构建与发布任务全绿；四个公开附件哈希及两种 macOS App/DMG 核验通过 | 完成；1.1.3 保留为注明包装问题的预发布，旧标签未删除 |
| remem | [PR1091](https://github.com/majiayu000/remem/pull/1091) 已合并 91e3ee0；#1090 已关闭；[v0.6.98](https://github.com/majiayu000/remem/releases/tag/v0.6.98) 已发布 | 最终 PR head a6b25c43、main CI、四平台 native 与 Windows 安全检查通过；修复 HTTP 测试分段读取，并把 graph/native 证据绑定 v093 | 完成；GitHub、crates.io、Homebrew、npm 0.6.98 已发布，MCP 注册为 active/latest；[release 36286249536](https://github.com/majiayu000/remem/actions/runs/36286249536) 全绿 |

## 性能与数据清理

5组交替Codex全量测试，应用缓存关闭、OS文件缓存保留：

- 耗时中位数：2.38s → 2.36s（0.992倍，满足规划≤1.05倍门槛）。
- 用户态CPU：6.35s → 6.73s（+6.0%）；总CPU中位数：8.92s → 9.29s（+4.1%）。
- RSS中位数：535.81MiB → 536.95MiB。
- Apple M1 Pro，8CPU、32GiB；用户游戏/桌面进程仍运行。本结果不推断空闲机器或其它平台表现。

真实会话副本、详细usage报表、私有stderr和基线cache/data已删除；仅保留匿名比较/计时结果。原始用户会话及有未提交改动的原始仓库未改动。

## 当前授权边界

2026-09-26 用户明确回复“这样做”，已批准本批合并、发布及本机 ccstats 更新；本批合并与发布已于 2026-09-27 完成。任何未通过依赖、当前head CI和review gates的仓库都不会因授权而跳过门槛。

详细过程与限制见WORKLOG.md；匿名结果在evidence/。
