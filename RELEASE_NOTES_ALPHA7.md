# MCDebugLauncher v26.0 Alpha 7（第一阶段）- Release Summary

## 发布日期
2026-08-09

## 版本信息
- **版本号**: v26.0.0-alpha.7
- **分支**: feature/despotes-integration
- **测试状态**: ✅ 全部通过 (33/33 tests，另 1 个真实下载集成测试默认禁用) + Despotes 端到端实测

---

## ✨ 本阶段核心：引入 Despotes，替换自制伴生模组

Alpha 7 是长程任务。第一阶段交付：把原自制伴生模组（mdl-agent-companion）**干掉**，全面接入你和社区同步开发的 [Despotes](https://github.com/NDBlockConnect/Despotes)。

### 1. Latest Release 包检测（按你的策略）

- 默认取 **Latest Release**（非预发布）中适用本实例（loader + MC 版本）的资产
- **Pre-Release 需要询问**：交互终端下明确 y/N 确认才安装；非交互会话（如 Agent API）不会自动装预发布
- **当没有适用的 Release 包则取最新的 Latest Pre-Release** 作为兜底候选（当前 v26.0 线仅有 Pre-Release，正走这条路径）
- 资产按 `Despotes-<tag>-<loader>-<mc>.jar` 命名解析，覆盖兼容矩阵：fabric 1.20-1.21.11（remapped）、1.20-1.20.6（legacy）、26.x（native 映射）

### 2. 创建实例自动扫模组包列表、输入序号选择

`mdl create` 完成后自动列出所有适用的 Despotes 包，编号展示（版本/类型/文件名/大小），用户输入序号选择安装；`0` 或回车默认 1；`[0]` 跳过。

```
mdl create myinst --mc-version 1.21.4 --loader fabric
# 交互：选择 [1] 安装的包
# 非交互：自动选最新适用 stable；pre 需显式 opt-in
```

新增标志：`--no-despotes`（跳过）、`--despotes-prerelease`（把预发布也列入可选列表）。

下载带 **sha256 校验**，缓存于 `<数据目录>/despotes/`，实例安装副本。

### 3. 运行时控制协议迁移到 Despotes HTTP

- `mdl game status/key/look/click/scroll/chat` 与 Agent API 的 `/api/v1/game/*` 全部改走 Despotes（`/despotes/v1/actions`、`/query`、`/screenshot`）
- 启动注入 `-Ddespotes.port`，端口发现文件 `runtime/despotes.port`
- 移除旧的 `mdl.agent.port` / `agent.port` / keepFocus 机制（Despotes 自带失焦截图与注入）

**端到端实测**（用户焦点全程不在游戏窗口）：Despotes 状态查询（windowFocused:false 仍可查）→ GUI 点击导航菜单 → 创建世界进入 → `look` 精确设朝向（yaw 90/pitch 10）→ `w` 移动（坐标按朝向正确变化）。

### 4. Fabric 实例自动安装 ModMenu

创建 Fabric 实例时顺带从 Modrinth 安装 [ModMenu](https://modrinth.com/mod/modmenu)（最佳努力，失败仅警告）。

### 5. 截图双通道

`mdl game screenshot` 优先 Despotes 帧缓冲（最小化也可用、纯客户区），失败回退 Windows.Graphics.Capture；输出标注来源。

---

## 🧪 测试验证

- 自动化：33/33 通过；新增 despotes 模块单测（解析/覆盖矩阵/选择策略）+ HTTP 信封单测
- 集成（默认禁用，`--ignored` 手动跑）：真实 GitHub 下载 → sha256 校验 → 副本安装
- 端到端：见上

## 📝 使用须知

- 旧实例若仍装 mdl-agent-companion，需手动移除并改用 Despotes（`mdl create` 会自动装）
- Pre-Release 包在交互终端需确认；非交互环境不自动安装预发布

---

**版本**: v26.0.0-alpha.7
**发布日期**: 2026-08-09
**下一阶段**: Alpha 7 后续（国内镜像测活、切片下载、模组/资源包/光影检索、日志增强、正版登录/皮肤、测试世界、JDK 自定义、动态内存/性能、Aprism、缓存制、BE、dll/exe 注入器）
