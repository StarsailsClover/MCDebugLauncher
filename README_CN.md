# MCDebugLauncher

专为快速测试、Mod 开发和 AI 代理自动化设计的命令行 Minecraft 启动器。支持所有主流 Mod 加载器（Forge、NeoForge、Fabric、Quilt、OptiFine）、全面的错误诊断，以及用于程序化控制的结构化输出。

[English Documentation](README.md)

## 特性

- **单命令启动**: 单条命令安装并启动任意 Minecraft 版本和任意 Mod 加载器
- **多加载器支持**: Vanilla、Forge、NeoForge、Fabric、Quilt、LegacyFabric、OptiFine、Aprism JE Native
- **Agent 游戏操控（Despotes）**: 不抢占焦点地观察与操控运行中的游戏——GPU 截图（Windows.Graphics.Capture + 游戏内帧缓冲）、输入注入（按键/鼠标/视角/聊天）、状态查询；切到别的应用游戏仍继续运行（自动处理 `pauseOnLostFocus`）
- **Modrinth 整合包导入**: `mdl import` 按整合包声明的版本/加载器创建实例，复制 overrides 并自动补全所有缺失文件（sha1 校验、幂等）
- **JE 专用服务器**: `mdl server create/launch/stop` 下载官方 server.jar、管理 eula/properties、后台运行
- **镜像与可靠下载**: 内置官方 + 国内镜像源实时测活、分块并行下载、自动换源、sha1 校验的 7 天副本安装缓存
- **内容搜索安装**: 一条命令从 Modrinth 搜索并安装 mod/资源包/光影
- **微软账号**: 设备码登录（无头友好）、账号列表、皮肤下载
- **完整性与自修复**: 启动前校验客户端 JAR、库与资源文件，损坏自动重新下载；启动时自动补装 Fabric API
- **智能诊断**: 自动崩溃报告收集、日志分析和错误检测
- **代理友好**: JSON 结构化输出，内置带启动进度与游戏就绪事件的 HTTP/WebSocket 服务器
- **实例管理**: Mod 管理、配置导入导出、世界备份恢复、可选启动队列（`--no-queue`）
- **中文本地化**: `--lang zh` 消息与 Windows UTF-8 控制台输出

## 快速开始

```bash
# 创建并启动 Fabric 实例（自动安装 Fabric API + ModMenu）
mdl create my-instance --mc-version 1.21.1 --loader fabric
mdl launch my-instance

# 导入 Modrinth 整合包（.mrpack）自动补全
mdl import my-pack ./cool-pack.mrpack

# 搜索并安装内容
mdl search mod sodium --mc-version 1.21.1 --loader fabric --instance my-instance

# 后台带 agent 控制启动，等待游戏就绪广播
mdl launch my-instance --detach --agent --wait-ready
mdl game status my-instance
mdl game screenshot my-instance --output shot.png

# JE 专用服务器
mdl server create my-server --mc-version 1.21.4
mdl server launch my-server
mdl server stop my-server

# Mod / 备份 / 诊断管理
mdl mod list my-instance
mdl backup create my-instance world1
mdl logs my-instance --follow
mdl diagnose my-instance --analyze
```

## Agent API

MCDebugLauncher 内置 HTTP/WebSocket 服务器，供 AI 代理和自动化工具进行程序化控制。

```bash
# 启动 agent 服务器
mdl agent --port 8080
```

**REST API:**
```bash
# 获取服务器状态
curl http://localhost:8080/api/v1/status

# 执行命令
curl -X POST http://localhost:8080/api/v1/execute \
  -H "Content-Type: application/json" \
  -d '{"command":"list","args":[],"options":{}}'
```

**WebSocket 事件流:**
```python
import asyncio
import websockets

async def listen():
    async with websockets.connect("ws://localhost:8080/api/v1/events") as ws:
        async for message in ws:
            event = json.loads(message)
            print(f"[{event['type']}] {event.get('message', '')}")
```

完整 API 文档请查看 [docs/specification.md](docs/specification.md)。

## 文档

- [规范文档](docs/specification.md) - 完整的 CLI 和 API 参考
- [研究文档](docs/RESEARCH_CN.md) - 技术分析和架构决策

## 状态

**当前版本**: v26.0 Alpha 8.1

- ✅ CLI 框架、版本管理、实例管理
- ✅ Forge / NeoForge / Fabric / Quilt / OptiFine 加载器
- ✅ 诊断、日志分析、agent HTTP/WebSocket 服务器
- ✅ Alpha 5: mod/配置/备份管理、自更新
- ✅ Alpha 6: agent 游戏操控（截图、输入注入、免焦点游玩）
- ✅ Alpha 7: Despotes 集成、镜像+分块下载+缓存、Modrinth 搜索、微软登录、Aprism JE、基岩版专用服、dll 注入器
- ✅ Alpha 8: 游戏就绪广播、启动前完整性校验、Fabric API 自修复、--no-queue、UTF-8 控制台修复
- ✅ Alpha 8.1: Modrinth 整合包导入（自动补全）、JE 专用服务器、启动更新纲要、detach 句柄泄漏修复

**已测试配置：**
- Vanilla Minecraft 1.21.x ✅
- Fabric Loader + Fabric API ✅
- Forge 52.x / NeoForge 21.x ✅
- 基岩版专用服 1.26.x ✅
- JE 专用服 1.21.4 ✅

## 贡献

欢迎贡献！在提交 Pull Request 之前，请阅读我们的贡献指南。

## 致谢

本项目在 AI（Claude）的协助下开发。技术研究和实施指导通过人机协作提供。

以下开源项目作为灵感和技术参考：
- [PrismLauncher](https://github.com/PrismLauncher/PrismLauncher) - 现代 Minecraft 启动器
- [PortableMC](https://github.com/mindstorm38/portablemc) - CLI 启动器设计模式
- [HeadlessMC](https://github.com/headlesshq/headlessmc) - 无头测试基础设施
- [MC-CLI](https://github.com/Th0rgal/mc-cli) - 代理控制接口

## 许可证

根据 Apache License 2.0 许可。详见 [LICENSE](LICENSE)。
