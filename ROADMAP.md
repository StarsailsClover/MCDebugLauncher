# MDL Roadmap

MCDebugLauncher 后续规划。版本命名沿用 Aprism 家族方案：每年一条主线（v26 = 2026），
每条主线含若干 minor（v26.0、v26.1 …），每个 minor 走 Alpha 预发布后收敛为正式版 Release。

> 状态图例：✅ 已完成 · 🔄 进行中 · 📋 规划中

## v26.0（已完成主线，收尾于 Alpha 12）

- ✅ Alpha 1–8.1：核心启动器、实例/模组管理、Agent 游戏控制（Despotes）、整合包导入、JE 服务端、启动更新摘要。
- ✅ Alpha 9：instance-info 命令、多语言、启动 changelog 精简。
- ✅ Alpha 10：Aprism 产品矩阵（loader / AprismRefract / AprismPrismate）+ Despotes vanilla。
- ✅ Alpha 11：真实下载进度条（单流 / 分块 / 资产批量）。
- ✅ Alpha 12：环境健康自检（`mdl doctor`）+ 本路线图。

## v26.1（已完成主线，收尾于 Alpha 5）

| Alpha | 主题 | 内容 |
|---|---|---|
| Alpha 1 | 更好的 AI 集成 | 结构化 Agent 命令面、面向 LLM 的稳定 JSON 契约、能力清单查询端点 |
| Alpha 2 | 更好的 Agent 使用体验 | 减少交互确认、幂等命令、更清晰的错误码、ready 事件增强 |
| Alpha 3 | BE 启动更多支持 | 在既有 BE 专用服基础上扩展启动/注入能力（配合 injector 基建） |
| Alpha 4 | 功能追平 | 对齐主流启动器的常见能力差距（以研究结论为准） |
| Alpha 5 | 稳定性与收敛 | 回归固化、文档、正式版发布 |

## v26.2（当前主线：自动化韧性与运维）

主题：让 MDL 在无人值守的 Agent 驱动场景下更可靠——进程生命周期自管、诊断自愈、网络容灾、可观测性。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | 进程生命周期与看门狗 | 空闲超时自动关闭：监控游戏日志输出，N 秒无新输出自动终止；`--idle-timeout <seconds>` 可配，`--no-idle-timeout` 禁用；Agent API `GET /api/v1/game/:instance/idle-status`；WebSocket `game_idle_timeout` 事件；白名单日志模式重置计时器 | ✅ 已完成 |
| Alpha 2 | 诊断系统补全 + 性能优化 | 实现 `log_parser.rs`（集成 mclog-analyzer）；crash report stack trace 提取；结构化 JSON 诊断输出；流式下载替代内存缓冲（修复 1.9GB 内存问题） | 📋 规划中 |
| Alpha 3 | Agent 命令面收敛 | 实现或移除 `agent/commands.rs`；统一 CLI/HTTP 命令映射；错误码一致性审计 | 📋 规划中 |
| Alpha 4 | 实例运维与导出 | `mdl export <instance> <output.mrpack>`；批量 mod 操作；磁盘占用报告；实例健康评分 | 📋 规划中 |
| Alpha 5 | 下载与网络容灾 | 镜像源健康度持续监测；自动换源；HTTP Range 断点续传；asset 预取优化 | 📋 规划中 |
| Alpha 6 | 微软账号与认证 | 多账号会话管理；token 自动刷新；皮肤下载与头像渲染 | 📋 规划中 |
| Alpha 7 | 测试世界与服务端自动化 | `--with-test-world` 超平坦世界；`--enter-world`/`--enter-server`；测试服生命周期自动化 | 📋 规划中 |
| Alpha 8 | Aprism BE Native 与跨平台 | Aprism BE Native 适配；Linux/macOS 构建验证；非 Windows 截图替代方案 | 📋 规划中 |
| Alpha 9 | 可观测性与遥测 | 结构化 JSON 日志；启动指标（time-to-ready、下载字节、缓存命中率）；`mdl metrics` 命令 | 📋 规划中 |
| Alpha 10 | 稳定性、文档与正式发布 | 零 warning 构建；全量文档刷新；回归测试套件；v26.2 正式版发布 | 📋 规划中 |

## 长期候选方向（未排期）

- 正版账号皮肤渲染、多账号会话管理增强。
- 测试世界 / 测试服务器自动化体验完善。
- Aprism BE Native 加载器的启动器侧适配。
- 更多镜像与下载源的健康度持续监测。
