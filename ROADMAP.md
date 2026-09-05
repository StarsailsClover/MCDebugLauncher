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

## v26.4（已完成主线：实测驱动修复 + Aprism 生态深化，收尾于 Alpha 10）

主题：由用户实测反馈、CI 首验与 fuzzer 发现驱动——每个 Alpha 都是真实问题的闭环。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | status 性能 + 评估修复 | `status` 全量路径单快照共享探测（p95 ~1.7-3.5s → **~0.2s**）；COM¹ 上标保留名绕过修复；execute usage 错误归类 BAD_REQUEST（F1/F2，见 ROBUSTNESS_V263.md） | ✅ 已完成 |
| Alpha 2 | mdl inject JVM 路由修复 | **Bug 修复**（用户实测）：`mdl inject --dll` 对 JVM 目标改走 JavaAgent + `System.load()` 路径——JDK 25 CFG/CET 缓解使 CreateRemoteThread 在 DllMain 前崩溃；嵌入 NativeLoaderAgent（premain+agentmain），运行时打包为最小 JAR 经 Attach API 加载；非 JVM 目标（bedrock_server.exe 等）保留旧路径 | ✅ 已完成 |
| Alpha 3 | Forge/NeoForge 判定修复 | **Bug 修复**（用户实测）：`build_classpath` 与 `add_game_arguments` 中 `is_neoforge` 改用 config `loader_type` 精确判定——旧 main_class 启发式（`bootstraplauncher` 子串）使 Forge 也命中 NeoForge 分支，触发假 version-mismatch bail（Forge version.json id 为 `1.20.1-forge-47.3.0` 无 `neoforge-` 前缀可剥）；附带清理 unused `main_class` 参数 warning | ✅ 已完成 |
| Alpha 4 | Linux/macOS CI 编译矩阵 | GitHub Actions `.github/workflows/ci.yml`：linux-x64 / aarch64-darwin / windows-msvc 三目标 check+test（`--locked` + rust-cache + `debuginfo=0`）。**首次非 Windows 编译验证落地**，途中修复两处平台泄漏：`is_jvm_process` 提升为可移植顶层函数（原困在 Windows 模块内，main.rs 无条件调用致 linux/darwin E0425）；`CHANGELOG.md` 从 .gitignore 移出并入库（`include_str!` 构建输入，干净检出缺文件全平台编译失败）。Cargo.lock 开始跟踪（二进制 crate 可复现构建）。macos-13 Intel 腿移除（镜像弃用、30 分钟无 runner）；附带清理 3 处测试 unused-import | ✅ 已完成 |
| Alpha 5 | cargo-fuzz 接入 CI + 首战告捷 | `fuzz/` 独立 workspace 三目标（`parse_log_content` 全链路解析 / `props_editor` 变异往返+不变量断言 / `jsonio_bom_parse` BOM+serde 镜像）；最小 parser-only `[lib]`（三文件零 crate 依赖原路径复用，main.rs 模块树不动）；`.github/workflows/fuzz.yml` 周期调度+手动触发+解析器 PR 门控，限时+RSS 上限+崩溃工件上传。**首次运行即挖出真 bug**：双 BOM（`EF BB BF ×2`）使 `strip_bom` 单层剥除后残留 U+FEFF 致 `parse_sync` 报晦涩错误——改为循环剥除并加回归测试；修复后三目标跑满预算零崩溃 | ✅ 已完成 |
| Alpha 6 | NeoForge MANIFEST 自动化 + AprismJDK 支持 | **NeoForge 26.x**：官方安装器产出的 patched-client 缺 `Minecraft-Dists: client` MANIFEST 属性（OpenLumin 手工步骤）——`install_loader` 后置钩子自动注入（26.x 门控、幂等、best-effort 不阻塞安装），manifest 合并器保留命名节、zip 原位重写经临时文件+rename。**AprismJDK（AJR）**：新模块 `loader/aprism_jdk.rs`（AprismLab/AprismJDK）——资产名解析（平台感知，拒收 agent jar）、stable-first 选择、流式下载 + SHA256SUMS.txt 校验、解压入 java cache；CLI `mdl jdk available/install/list/remove`；launch `--jdk aprism[@ver]`（与 `--java-path` 互斥）；`aprism status` 增 JDK 行。**实测闭环**：真实下载 265MB v26.2（Java 25.2.1），SHA256 通过，launch 解析与生态视图验证 | ✅ 已完成 |
| Alpha 7 | Despotes v26.9 封装 + JDK 回退 | **v26.9 四原语映射**：`mdl game redstone`（坐标可选，缺省十字准星探测）、`schedule add/status/remove`（--period-ticks + 可重复 --command JSON）、`macro` 七操作（start/record-step/stop-recording/play/stop/delete/status）、`condition`（--if/--then/--else，点路径字段 + 六比较符）；另设 `raw-action` 透传通道保证向前兼容；client.rs 新增 payload 构造器与 automation_action 通道。**JDK 回退**：`launch --jdk aprism[@ver]` 解析失败不再硬错——打印原因后回退系统 Java / Eclipse Adoptium 自动供给链。离线验证全部参数校验路径 + 回退路径实测 | ✅ 已完成 |
| Alpha 8 | Agent API 暴露 v26.9 原语 | `POST /game/:instance/input` 新增 schedule/macro/condition/raw-action 四输入类型（服务端侧校验与 CLI 对齐：op 白名单、name/step 必填规则）；新增 `POST /game/:instance/redstone`（body 可选，缺省十字准星探测）；capabilities 清单同步（endpoints + game_inputs + 测试断言扩展）；docs/AGENT_API.md 补 JSONC 配方。AI agent 现在可经 HTTP 直接编排周期任务/宏/条件分支/红石感知 | ✅ 已完成 |
| Alpha 9 | Agent launch 支持 jdk + 文档刷新 | execute `launch` 新增 `jdk aprism[@ver]` 选项（与 java-path 互斥校验、解析失败回退 Adoptium 并记入事件流）；capabilities 清单与 AGENT_API.md 同步；README/README_CN 状态区与亮点刷新至 v26.4（新增 Aprism 生态条目、跨平台 CI/fuzz、MANIFEST 自动注入） | ✅ 已完成 |
| Alpha 10 | LTS 收敛 | 版本转正 26.4.0；CHANGELOG 官方条目（Alpha 1–9 全记录）；回归验证：28 lib + 174 bin 测试、三平台 CI 绿、fuzz 三目标预算内零崩溃、capabilities 完整性断言通过 | ✅ 已完成 |

## v26.5（主线：自主编排与运行时选择；独立分支开发，起点 ROBUSTNESS_V264.md）

主题：面向 AI agent 的自主编排深化——实例级运行时绑定、编排能力复合与
Agent API 一致性；ROBUSTNESS_V264 全部发现（F1–F6）作为鲁棒性工作纳入；
自本版起执行分支开发与开发水印规范（GitHub@NDBlockConnect）。

| Alpha | 主题 | 内容 | 状态 |
|---|---|---|---|
| Alpha 1 | jdk 删除穿越修复（F1/F2） | **F1 [Medium-High]**：`jdk remove` 路径穿越（PoC 确证缓存外哨兵被删）——tag 仅允许匹配 `installed()` 枚举条目 + 拒绝分隔符/父级组件；**F2 [Low-Medium]**：下载归档名 `archives.join(asset.name)` 拒绝含路径分隔符的资产名；两者均带回归测试 + 水印合规 | ✅ 已完成 |
| Alpha 2 | Agent API 错误面收敛（F3/F4/F5）+ bench 基线（F6） | 提取器拒绝统一 JSON envelope；input 校验错误 4xx 归类；redstone 坏 body 显式 400；空闲环境落 `perf-bench.ps1 -Baseline` | 📋 规划中 |
| Alpha 3 | 实例级 JDK 绑定 | `mdl jdk use <instance> [spec]`：spec = `aprism`/`aprism@<tag\|ver>`/`default`（清除）/省略（查看）；绑定持久化于 instance.json `jdk` 字段（serde default 保证旧配置兼容，round-trip 测试）；launch 决策链升级为三级——`--java-path/--jdk`（单次）→ 实例绑定（不可解析时 WARN 降级 Adoptium，同 alpha.1 语义）→ 自动供给；doctor 新增 `jdk-bindings` 检查（未解析绑定记 WARN 不判 FAIL）；`InstanceManager::update_config` 通用读改写；实测全生命周期（set/show/config 落盘/clear/doctor）。**事故记录**：PowerShell Set-Content 双重编码损坏 main.rs——git 恢复后全用 Edit 工具重做，规范永久禁用该路径（见 FACT.md 2026-08-31） | ✅ 已完成 |
| Alpha 4 | Despotes v26.11 原语映射 | `mdl game circuit`（立方体元件扫描 radius 1-8，缺省十字准星）、`game redstone-action`（toggle/cycle 元件交互，face/count 可选）、`game screen`（窗口几何块，physical/guiScale=logical 换算）；Agent API 同步：`redstone-action` 输入类型（复用 CLI 构造器校验→400）+ `POST /circuit`（坏 body 显式 400，沿袭 F5 语义）+ `GET /screen`；capabilities + AGENT_API.md 同步；JSON 形状取自 v26.11 官方 Release Notes（实测证据：343 方块扫描、拉杆/音符盒交互） | ✅ 已完成 |
| Alpha 5 | WS 编排事件流 | agent server 内置 orchestration watcher（5s 轮询 tracked 游戏的 Despotes schedule status，diff 后广播 `schedule_registered`/`schedule_fired`/`schedule_removed`）——agent 从轮询编排状态转为事件驱动响应；响应形状取自 Despotes 源码 `ScheduleManager.statusJson()`（权威证据，pin 测试锁定）；diff 纯函数化 + 快照语义（瞬时失败不产生幻影 removed；游戏失联清空快照自然重注册）；capabilities 事件清单 + AGENT_API.md 同步；watcher 故障绝不拖垮 server | ✅ 已完成 |
| Alpha 6 | macro 生命周期事件流 | 编排 watcher 扩展至 macro 状态（MacroRecorder.statusJson 形状自 Despotes 源码 pin 测试锁定）：`macro_recorded`（录制完成入列）/`macro_play_started`（含总步数）/`macro_play_finished`/`macro_removed`；播放中换宏 emitting finish+start 保序；与 schedule 轮询同循环同快照语义（瞬时失败不幻影、失联清空）；capabilities + AGENT_API.md 同步 | ✅ 已完成 |
| Alpha 7 | **Bug 修复**：instance→window 映射穿越 | **字段报告**（alpha.5 期间观察）：openlumin 游戏错误画面被映射到另一实例名——`collect_running_pids` 无条件信任 runtime/pid 文件，游戏崩溃后文件残留 + Windows PID 复用 → 他实例 java 进程持有该 pid → Match 2/Path 1 错误归属（且 `find_for_instance` 强制改写合成名加重错配）。修复三重校验：①pid 存活且为 java/javaw 且命令行含 `instances/<name>` gameDir 标记（边界字符校验防前缀混淆，纯函数测试锁定）；②同 pid 被多实例声明即歧义整体丢弃；③`find_for_instance` Path 1 身份校验失败降级标题匹配。**环境发现**：本机 IPv6 路由失效（ping -6 100% 丢包）+ DNS AAAA 优先 → reqwest 30s 超时而 curl 正常（test_fetch_manifest 本地失败、CI 绿，判定环境非代码） | ✅ 已完成 |
| Alpha 8 | circuit 变化事件 / watch API | API 内存订阅：`POST/GET/DELETE /game/:instance/watch` 注册命名 cube（x/y/z 必填，radius 1-8）；watcher 仅轮询 tracked 游戏的订阅，WorldProbes.circuit 权威响应形状 pin 测试锁定，按位置 diff 后广播 `circuit_changed`（appeared/changed/removed，变化列表上限 64）；菜单 `inWorld=false` 不制造 mass removals，删除 watch 清理快照确保同名重注册重新触发；不修改游戏配置、server 重启即丢弃订阅 | ✅ 已完成 |
| Alpha 9–10 | 待定（按使用反馈） | 编排复合 DSL、更多生态候选 | 📋 规划中 |

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
