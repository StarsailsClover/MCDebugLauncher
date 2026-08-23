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

## v26.3（当前主线：加固与 Agent 面）

主题：消化 v26.2 鲁棒性评估发现（F1/F3/F4，见 ROBUSTNESS_V262.md），补全 Agent REST/execute 能力面。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | 输入校验 + 配置容错 + Agent API 补全 | 名称白名单校验（create/rename/clone/server create；保留设备名黑名单、路径分隔符、尾点/尾空格）；BOM 容错 JSON 读取助手覆盖 instance/server/account/metrics/javaagent 全部配置点且报错含文件路径；attach 错误归因修正（非 JVM ≠ 模块缺失）；REST `/api/v1/instance/:name/metrics`+`/disk`；execute 新增 `metrics`/`disk`/`inject-agent`/`server-cmd` 映射；capabilities 同步 | ✅ 已完成 |
| Alpha 2 | OOM 二次确认 + 跨平台矩阵 | 杀进程前列出候选（PID/进程名/内存/窗口标题）并按 `--oom-confirm auto\|always\|never` 门控（auto 仅交互终端提示）；`--oom-list-only` 干跑枚举；agent execute 透传 `oom-confirm`/`oom-list-only`；docs/PLATFORM_MATRIX.md 平台门控审计（linux/macos 编译验证待 CI，宿主镜像 404 阻断） | ✅ 已完成 |
| Alpha 3 | 诊断增强 | `DiagnosticReport` 新增 `idle_timeout_event`（runtime/idle_timeout 标记解析）与 `last_launch_metrics`；崩溃存在时输出关联启发式（watchdog 挂死签名 / 从未就绪即崩 / 同日时间链接）；文本与 JSON 导出同步渲染 | ✅ 已完成 |
| Alpha 4 | 服务端深化 | `server.properties` 结构化编辑器 `mdl server props list/get/set`（注释与顺序保留、重复键折叠）；封装命令：`allowlist add/remove/list/enable/disable`（RCON 运行态 + 停止态文件回退 + 属性开关）、`op add/remove/list`、`ban add/pardon/list`（列表走 JSON 文件，停止态可用） | ✅ 已完成 |
| Alpha 5 | 安全加固 | accounts token 文件权限收紧；RCON 密码轮换命令 | 📋 规划中 |
| Alpha 6 | 性能基线 | 启动耗时基准脚本 + 回归阈值门 | 📋 规划中 |
| Alpha 7 | 文档刷新 | AGENT_API/specification 与 v26.3 面对齐；examples 更新 | 📋 规划中 |
| Alpha 8 | 解析器模糊测试 | cargo-fuzz 关键解析路径（mrpack/log/manifest） | 📋 规划中 |
| Alpha 9 | 可用性打磨 | i18n 补全；交互向导优化 | 📋 规划中 |
| Alpha 10 | LTS 收敛 | 回归套件固化；v26.3 正式版发布 | 📋 规划中 |

## v26.2（已完成主线：自动化韧性与运维，收尾于 Alpha 10）

主题：让 MDL 在无人值守的 Agent 驱动场景下更可靠——进程生命周期自管、诊断自愈、网络容灾、可观测性。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | 进程生命周期与看门狗 | 空闲超时自动关闭：监控游戏日志输出，N 秒无新输出自动终止；`--idle-timeout <seconds>` 可配，`--no-idle-timeout` 禁用；Agent API `GET /api/v1/game/:instance/idle-status`；WebSocket `game_idle_timeout` 事件；白名单日志模式重置计时器 | ✅ 已完成 |
| Alpha 2 | 诊断系统补全 + 性能优化 | 实现 `log_parser.rs`（集成 mclog-analyzer）；crash report stack trace 提取；结构化 JSON 诊断输出；流式下载替代内存缓冲（修复 1.9GB 内存问题） | ✅ 已完成 |
| Alpha 3 | Agent 命令面收敛 | 实现或移除 `agent/commands.rs`；统一 CLI/HTTP 命令映射；错误码一致性审计 | ✅ 已完成 |
| Alpha 4 | OOM 自阻止与实例运维 | 启动前 OOM 自保护：杀死残留 Minecraft/Java 进程、裁剪系统 Working Set（EmptyWorkingSet）、可选清空 Standby List（NtSetSystemInformation）；`--oom-protect`/`--oom-aggressive` CLI 参数；Agent API `oom-protect`/`oom-aggressive` 选项 | ✅ 已完成 |
| Alpha 5 | 实例运维与导出 | `mdl instance export --format mrpack`（modrinth.index.json + overrides，与 import 成往返）；`mdl javaagent` 实例级 agent 管理；启动时 `--javaagent <jar>[=params]` 可重复注入 | ✅ 已完成 |
| Alpha 6 | 指定 Agent 注入 + 账号会话 | `mdl game inject-agent <instance> <jar>` 运行中 JVM 热附加（内嵌 AttachHelper + jdk.attach/agentmain）；`mdl account refresh [--all]` token 刷新（登录保存 refresh_token）；`mdl status --disk` 磁盘占用报告（单实例含子目录分解） | ✅ 已完成 |
| Alpha 7 | 测试世界与服务端自动化 | `mdl server launch --wait-ready`（日志 Done 行轮询）；RCON 集成（create 自动启用，密码入 server.json）；`mdl server stop` 优雅停止（RCON stop → 20s 等待 → taskkill 兜底）；`mdl server cmd <name> <cmd>` 控制台命令通道；`--enter-test-world` 自适应补全（创建确认按钮 + 已有世界 Play 路径 + inGame 终态确认） | ✅ 已完成 |
| Alpha 8 | Aprism 生态联动 + 已有功能优化 | `mdl aprism status <instance>` 统一生态视图（agent 缓存/覆盖、Refract .aep、Prismate、.aje 原生模组、互斥提示，纯离线）；mod 管理支持 `.aje` 原生模组（install 校验 + list kind 字段）；debug 构建 `--help` 栈溢出修复（32MB 主线程栈）；编译 warning 清零 | ✅ 已完成 |
| Alpha 9 | 可观测性 + Refract/Prismate 支持 | `--log-format json` 结构化日志（tracing json 格式）；启动指标采集（spawn/ready 耗时、下载字节、缓存命中率）落盘 `runtime/metrics.json(.jsonl)`；`mdl metrics <instance> [--history]`；`aprism refract remove`/`aprism prismate remove` 补全生命周期 | ✅ 已完成 |
| Alpha 10 | LTS 收敛 | 文档全量刷新（README 状态区重写）；回归验证（116 tests / capabilities / doctor 8/8 / 零 warning） | ✅ 已完成 |

## 长期候选方向（未排期）

- 正版账号皮肤渲染、多账号会话管理增强。
- 测试世界 / 测试服务器自动化体验完善。
- Aprism BE Native 加载器的启动器侧适配（**当前冻结，待 Aprism BE 解冻后启动**）。
- 更多镜像与下载源的健康度持续监测。
