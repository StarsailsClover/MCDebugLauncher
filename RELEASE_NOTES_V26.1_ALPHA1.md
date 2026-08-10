# MDL v26.1 Alpha 1 — Better AI Integration

## Theme

v26.1 主线第一个 Alpha：让 MDL 对 LLM / Agent 更友好。核心交付是**机器可读的能力清单**，
AI agent 无需解析帮助文本即可发现 MDL 的完整命令面。

## 1. 能力清单（Capability Manifest）

新增两种访问方式，返回同一份机器可读清单（schema `mdl.capabilities/v1`，additive-only 契约）：

- `GET /api/v1/capabilities` — Agent 服务器 REST 端点
- `mdl capabilities` — CLI 命令

清单内容：
- 全部 `/api/v1/*` REST 端点（method + path + 用途）
- 全部 `execute` 命令（参数 + 选项）：list / create / info / launch
- 全部游戏输入类型：key / look / click / scroll / chat（含字段说明）
- WebSocket 事件流契约（事件种类）

## 2. Agent launch 选项扩展

`POST /api/v1/execute` 的 `launch` 命令现在额外支持 options：
`java-path`、`memory`、`aprism`、`enter-test-world`。

## 3. 机器可读命令的纯净输出

`capabilities` 与 `--format json` 一律抑制 stdout 日志与启动横幅，保证输出可被干净解析。

## 实测

- `mdl capabilities` 输出纯净 JSON（schema=mdl.capabilities/v1，9 endpoints，4 execute commands，5 game inputs）
- `GET /api/v1/capabilities` HTTP 端点返回有效 JSON
- 70 项单元测试全绿（新增 3 项能力清单测试）

## Files

- `mdl.exe` (Windows x64 release build, v26.1.0-alpha.1)
- See `CHANGELOG.md` for the detailed changelog entry.
