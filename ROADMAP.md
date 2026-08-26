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

## v26.4（当前主线：性能与鲁棒性跟进）

主题：消化 v26.3 鲁棒性评估（ROBUSTNESS_V263.md）与 alpha.6 性能发现，补齐跨平台与生态跟进。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | status 性能 + 评估修复 | `status` 全量路径单快照共享探测（p95 ~1.7-3.5s → **~0.2s**）；COM¹ 上标保留名绕过修复；execute usage 错误归类 BAD_REQUEST（F1/F2，见 ROBUSTNESS_V263.md） | ✅ 已完成 |
| Alpha 2 | mdl inject JVM 路由修复 | **Bug 修复**（用户实测）：`mdl inject --dll` 对 JVM 目标改走 JavaAgent + `System.load()` 路径——JDK 25 CFG/CET 缓解使 CreateRemoteThread 在 DllMain 前崩溃；嵌入 NativeLoaderAgent（premain+agentmain），运行时打包为最小 JAR 经 Attach API 加载；非 JVM 目标（bedrock_server.exe 等）保留旧路径 | ✅ 已完成 |
| Alpha 3 | Forge/NeoForge 判定修复 | **Bug 修复**（用户实测）：`build_classpath` 与 `add_game_arguments` 中 `is_neoforge` 改用 config `loader_type` 精确判定——旧 main_class 启发式（`bootstraplauncher` 子串）使 Forge 也命中 NeoForge 分支，触发假 version-mismatch bail（Forge version.json id 为 `1.20.1-forge-47.3.0` 无 `neoforge-` 前缀可剥）；附带清理 unused `main_class` 参数 warning | ✅ 已完成 |
| Alpha 4 | Linux/macOS CI 编译矩阵 | GitHub Actions `.github/workflows/ci.yml`：linux-x64 / aarch64-darwin / windows-msvc 三目标 check+test（`--locked` + rust-cache + `debuginfo=0`）。**首次非 Windows 编译验证落地**，途中修复两处平台泄漏：`is_jvm_process` 提升为可移植顶层函数（原困在 Windows 模块内，main.rs 无条件调用致 linux/darwin E0425）；`CHANGELOG.md` 从 .gitignore 移出并入库（`include_str!` 构建输入，干净检出缺文件全平台编译失败）。Cargo.lock 开始跟踪（二进制 crate 可复现构建）。macos-13 Intel 腿移除（镜像弃用、30 分钟无 runner）；附带清理 3 处测试 unused-import | ✅ 已完成 |
| Alpha 5 | cargo-fuzz 接入 CI + 首战告捷 | `fuzz/` 独立 workspace 三目标（`parse_log_content` 全链路解析 / `props_editor` 变异往返+不变量断言 / `jsonio_bom_parse` BOM+serde 镜像）；最小 parser-only `[lib]`（三文件零 crate 依赖原路径复用，main.rs 模块树不动）；`.github/workflows/fuzz.yml` 周期调度+手动触发+解析器 PR 门控，限时+RSS 上限+崩溃工件上传。**首次运行即挖出真 bug**：双 BOM（`EF BB BF ×2`）使 `strip_bom` 单层剥除后残留 U+FEFF 致 `parse_sync` 报晦涩错误——改为循环剥除并加回归测试；修复后三目标跑满预算零崩溃 | ✅ 已完成 |
| Alpha 6 | NeoForge MANIFEST 自动化 | 26.x patched-client 自动注入 `Minecraft-Dists: client`（OpenLumin 手工步骤消除） | 📋 规划中 |
| Alpha 7 | Despotes v26.9 封装 | schedule/macro/condition/redstone 的 mdl game 子命令映射 | 📋 规划中 |
| Alpha 8–10 | 待定（按使用反馈） | — | 📋 规划中 |

## v26.3（已完成主线：加固与 Agent 面，收尾于 Alpha 10）

主题：消化 v26.2 鲁棒性评估发现（F1/F3/F4，见 ROBUSTNESS_V262.md），补全 Agent REST/execute 能力面。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | 输入校验 + 配置容错 + Agent API 补全 | 名称白名单校验（create/rename/clone/server create；保留设备名黑名单、路径分隔符、尾点/尾空格）；BOM 容错 JSON 读取助手覆盖 instance/server/account/metrics/javaagent 全部配置点且报错含文件路径；attach 错误归因修正（非 JVM ≠ 模块缺失）；REST `/api/v1/instance/:name/metrics`+`/disk`；execute 新增 `metrics`/`disk`/`inject-agent`/`server-cmd` 映射；capabilities 同步 | ✅ 已完成 |
| Alpha 2 | OOM 二次确认 + 跨平台矩阵 | 杀进程前列出候选（PID/进程名/内存/窗口标题）并按 `--oom-confirm auto\|always\|never` 门控（auto 仅交互终端提示）；`--oom-list-only` 干跑枚举；agent execute 透传 `oom-confirm`/`oom-list-only`；docs/PLATFORM_MATRIX.md 平台门控审计（linux/macos 编译验证待 CI，宿主镜像 404 阻断） | ✅ 已完成 |
| Alpha 3 | 诊断增强 | `DiagnosticReport` 新增 `idle_timeout_event`（runtime/idle_timeout 标记解析）与 `last_launch_metrics`；崩溃存在时输出关联启发式（watchdog 挂死签名 / 从未就绪即崩 / 同日时间链接）；文本与 JSON 导出同步渲染 | ✅ 已完成 |
| Alpha 4 | 服务端深化 | `server.properties` 结构化编辑器 `mdl server props list/get/set`（注释与顺序保留、重复键折叠）；封装命令：`allowlist add/remove/list/enable/disable`（RCON 运行态 + 停止态文件回退 + 属性开关）、`op add/remove/list`、`ban add/pardon/list`（列表走 JSON 文件，停止态可用） | ✅ 已完成 |
| Alpha 5 | OOM 误杀修复 + 安全加固 | **修复误杀**：正向匹配收紧为强启动标记（net.minecraft./cpw.mods./--gameDir/fabricloader/neoforge- 等 12 项），工作区路径含 "Minecraft" 的 Kotlin 编译守护/JPS 构建/Maven fork/JDT 不再中招；排除表补充 kotlincompiledaemon/org.jetbrains.jps/org.eclipse.jdt/maven/surefirebooter；java/javaw 一律走命令行判定（不再走原生名直通）。**安全加固**：accounts 凭据文件 ACL 收紧（Windows 去继承仅留当前用户 / Unix 600，存量文件幂等回补）；`mdl server rotate-rcon [--show]` 密码轮换同步 props+json | ✅ 已完成 |
| Alpha 6 | 性能基线 | `mdl bench cli [--iterations N]` 自冷延迟基准（capabilities/status/list，min/p50/p95/max）；`scripts/perf-bench.ps1` 基线保存/回归门控（p95 超基线 ×容差即 exit 2）+ 可选实例 metrics 历程汇总；**发现**：`status` 全量 62 实例 p95 达 ~1.7s（逐实例 sysinfo 探测），列入后续优化项 | ✅ 已完成 |
| Alpha 7 | 文档刷新 | `docs/AGENT_API.md` 重写为 v26.3 权威紧凑参考（12 端点/9 execute 命令/8 事件种/11 错误码 + 三语言配方）；specification.md Agent 段修正（移除虚构的 `agent start`/auth-token/version 0.1.0，对齐真实端点表与事件集并指向 capabilities 为准）；examples 双语言演示重写（capabilities→launch→ready→input→idle/metrics/disk→stop 全链路，语法校验通过） | ✅ 已完成 |
| Alpha 8 | 看门狗竞态修复 + 对抗性测试套件 | **修复 OpenLumin 实测阻塞**：`--detach --wait-ready` 下看门狗不再先行武装——挂起载荷 `runtime/watchdog_pending.json` 落盘，就绪后由 cmd_launch 武装；未就绪则丢弃（不杀悬挂中的启动过程供诊断）。对抗性解析套件：log_parser 11 种垃圾输入含 20 万字符行、props 编辑器 55 行畸形键值——零 panic。cargo-fuzz 因宿主无 nightly 记为 CI 待办。**环境事故披露**：C 盘满（0 字节）致链接器 PDB 失败，清理 target(25.1GB)+27 个历史 zip 后恢复，测试曾实弹误杀构建守护进程一事已在 alpha.5 披露并整改 | ✅ 已完成 |
| Alpha 9 | 可用性打磨（扫描驱动） | doctor 新增 `mdl-on-path` 检查：多套 mdl 并存时 WARN 列出全部路径（源自 OpenLumin 手动换 zip 与 Downloads 双拷贝痛点）；`create --mc` 作为 `--mc-version` 别名落地（OpenLumin 文档实际写法） | ✅ 已完成 |
| Alpha 10 | LTS 收敛 | README/README_CN 横幅与亮点定稿；本地 CHANGELOG 26.3.0 官方条目；回归验证（159 tests / doctor 9 项 / capabilities） | ✅ 已完成 |

## v26.4（候选方向，未排期）

基于 Alpha 8 竞态修复与 workspace 扫描的后续候选：
- `status` 性能优化（逐实例 sysinfo 探测致 p95 ~1.7s，见 alpha.6 发现）
- Linux/macOS CI 编译矩阵落地（PLATFORM_MATRIX.md 待办）
- cargo-fuzz 解析器模糊测试接入 CI
- NeoForge 26.2 patched-client MANIFEST 注入自动化（OpenLumin 手工步骤）
- Despotes v26.9 automation primitives 的 MDL 侧封装

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
