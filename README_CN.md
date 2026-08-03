# MCDebugLauncher

专为快速测试、Mod 开发和 AI 代理自动化设计的命令行 Minecraft 启动器。支持所有主流 Mod 加载器（Forge、NeoForge、Fabric、Quilt、OptiFine）、全面的错误诊断，以及用于程序化控制的结构化输出。

[English Documentation](README.md)

## 特性

- **单命令启动**: 单条命令安装并启动任意 Minecraft 版本和任意 Mod 加载器
- **多加载器支持**: Vanilla、Forge、NeoForge、Fabric、Quilt、LegacyFabric、OptiFine
- **智能诊断**: 自动崩溃报告收集、日志分析和错误检测
- **代理友好**: 为 AI 代理和自动化工具提供 JSON 结构化输出
- **开发者工具**: 调试日志、性能分析、无头模式支持
- **实例隔离**: 独立实例，拥有独立的 mod、配置和 Java 版本
- **自动 Java 检测**: 检测 Java 版本要求并提供升级指引
- **实例管理**: Mod 管理、配置导入导出、世界备份恢复
- **企业级规范**: 生产级代码质量与全面的错误处理

## 快速开始

```bash
# 创建并启动 Fabric 实例
mdl create my-instance --mc-version 1.21.1 --loader fabric
mdl launch my-instance

# 管理 mod
mdl mod list my-instance
mdl mod install my-instance fabric-api-0.92.0.jar

# 备份和恢复世界
mdl backup create my-instance world1
mdl backup list my-instance
mdl backup restore my-instance world1_20260727_120000

# 查看日志和诊断信息
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
