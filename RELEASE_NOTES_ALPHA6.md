# MCDebugLauncher v26.0 Alpha 6 - Release Summary

## 发布日期
2026-08-08

## 版本信息
- **版本号**: v26.0.0-alpha.6
- **二进制大小**: 见发布资产（release 构建，LTO 优化 + strip）
- **分支**: bugfix/neoforge-classpath-and-features
- **测试状态**: ✅ 全部通过 (28/28 tests) + 真实游戏实例端到端验证

---

## ✨ 核心亮点：Agent 游戏操控能力

Alpha 6 让 Agent 第一次真正具备"操作 Minecraft"的能力，且满足三个硬约束：
**高性能实时截图**、**不占用用户的鼠标键盘**、**用户把焦点放在别的应用时游戏不弹暂停菜单**。

### 1. 高性能实时截图（Windows.Graphics.Capture）

- 基于 Windows.Graphics.Capture API 的按窗口捕获，GPU 加速
- **失焦、被遮挡、最小化以外的后台状态均可截图**，完全不需要激活游戏窗口
- 实测单帧往返约 0.6~0.8s（856×519）
- 窗口定位以**进程 PID 优先**（游戏标题参数可能被客户端忽略），标题前缀 `MDL: <实例名>` 为兜底

```bash
mdl game screenshot <instance> --output shot.png        # CLI
GET  /api/v1/game/<instance>/screenshot                 # 原始 PNG
GET  /api/v1/game/<instance>/screenshot?base64=true     # JSON base64
```

### 2. 游戏内输入注入（伴生模组 mdl-agent-companion）

调研证实：纯外部消息注入（PostMessage/SendInput）对 Minecraft Java 版**不可靠**——游戏逻辑层有 `isWindowFocused` 门控，失焦时移动/使用操作被直接忽略，且 SendInput 会抢占用户真实鼠标键盘。

因此 Alpha 6 采用**伴生模组**架构：MDL 随包附带轻量 Fabric 模组，`--agent` 启动时自动装入实例，在游戏进程内部注入输入——走 Minecraft 自己的 keybinding/界面系统，**与真实输入同一条处理路径**，完全不碰用户键鼠、不需要窗口焦点。

```bash
mdl game key    <instance> w --action press|release|tap   # 移动/跳跃/潜行等
mdl game look   <instance> --yaw 0 --pitch 0 [--relative] # 视角旋转
mdl game click  <instance> --x <guiX> --y <guiY>          # 鼠标/GUI 点击
mdl game scroll <instance> 1                              # 滚轮切快捷栏
mdl game chat   <instance> "/time set day"                # 聊天/命令
mdl game status <instance>                                # 状态查询
```

对应 REST：`POST /api/v1/game/<instance>/input`（`{"type":"key"|"look"|"click"|"scroll"|"chat", ...}`）、`GET .../status`。

**端到端实测通过**（用户焦点全程不在游戏窗口）：主菜单导航 → 进入世界选择 → 加载进入世界 → 世界内 `look` 改朝向、`w` 移动（坐标按预期变化）、`/time set day` 执行成功。

### 3. 防暂停（失焦不弹暂停菜单）

- Agent 启动时自动写入 `options.txt` 的 `pauseOnLostFocus:false`
- 同时注入 `mdl.agent.keepFocus=true` JVM 属性，伴生模组以 Mixin 让游戏认为窗口始终聚焦，击败游戏内部的焦点门控
- 两者叠加后，用户切到任何别的应用，游戏继续运行、Agent 输入继续生效

### 4. 通信协议（安全边界）

- 本地 TCP（127.0.0.1），默认端口 25590，JSON 行协议，协议版本 v1
- 实际监听端口写入 `<实例>/runtime/agent.port` 供启动器发现
- 仅回环，不暴露到网络

---

## 🔧 既有功能修复与增强

### 5. 后台启动（--detach）真正可用
- `mdl launch <instance> --detach` 立即返回并报告真实 PID
- 游戏输出重定向到 `logs/launch_detached.log`
- 启动锁移交给游戏进程，单实例排队保障依旧成立

### 6. Agent API launch 不再阻塞
- 旧实现的 `POST /api/v1/execute {"command":"launch"}` 会挂到游戏退出且 PID 恒报 0
- 现在改为后台启动并立即返回真实 PID；服务器跟踪运行实例并新增 `instance_stopped` 事件

### 7. 命令退出挂起修复
- 网络异常（如 GitHub 更新检查遇到无响应网络）时，CLI 命令在退出阶段可能卡住数分钟
- 更新检查加 8s 硬超时、HTTP 客户端加 10s 连接超时，且命令结束时显式退出进程
- 实测普通命令 0.97s 返回

### 8. 窗口发现 PID 优先
- 部分客户端会忽略 `--title` 参数导致按标题找不到游戏窗口
- 现在以实例 PID 匹配为主、标题前缀匹配为辅；`mdl game windows` 同样升级

---

## 📦 发布资产

```
release/mdl-v26.0-alpha.6-windows-x64/
├── mdl.exe                              (LTO + strip)
├── mdl-agent-companion-1.0.0.jar        (Alpha 6 伴生模组，随包附带)
├── README.md / README_CN.md
├── CHANGELOG.md
├── LICENSE
└── RELEASE_NOTES_ALPHA6.md
```

伴生模组查找顺序：与 mdl.exe 同目录 → 当前工作目录 → MDL 数据目录的 `companions/`。

## 🧪 测试验证

- 自动化测试：28/28 通过（新增 game 模块单元/集成测试 10 个）
- 伴生模组经 Gradle + Loom 构建（JDK 21，Loom 1.9.2，yarn 1.21.4+build.8）
- 手动/端到端（bc-test 实例，Fabric 1.21.4）：截图、GUI 点击、世界进入、移动/视角/聊天/滚轮、防暂停、失焦截图全部验证

## 📝 使用须知

- Agent 操控仅支持 **Fabric/Quilt** 实例（伴生模组基于 Fabric Loader + Mixin）；Vanilla/Forge/NeoForge 实例仍可使用截图与启动/诊断等能力，但输入注入不可用（启动时会明确提示）
- 截图仅支持 Windows（依赖 Windows.Graphics.Capture）

---

**版本**: v26.0.0-alpha.6
**发布日期**: 2026-08-08
**下一版本**: v26.0.0-alpha.7 / Beta 1 (TUI 规划中)
