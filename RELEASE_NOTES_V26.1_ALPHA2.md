# MDL v26.1 Alpha 2 — Better Agent Experience

## Theme

v26.1 主线第二个 Alpha：让 Agent 对 MDL 的控制闭环更完整、错误处理更可机读。

## 1. `stop` execute 命令

此前 agent 能通过 `POST /api/v1/execute {command:"launch"}` 启动实例，却没有对应的停止能力。
新增 `stop`：

- `POST /api/v1/execute {command:"stop", args:[name]}`
- 杀死实例的游戏进程树（taskkill /T /F），清理 PID 文件，更新服务器运行状态表，并广播 `instance_stopped` 事件。

## 2. 机器可读错误码

此前 execute 失败只返回英文错误文本 + 一律 HTTP 500，agent 无法可靠分支。现在每个失败响应携带：

- `error_code` 字段：`UNKNOWN_COMMAND` / `BAD_REQUEST` / `NOT_FOUND` / `ALREADY_EXISTS` / `NOT_RUNNING` / `BUSY` / `INTERNAL`
- 匹配的 HTTP 状态码：400 / 404 / 409 / 500

## 3. 能力清单同步更新

`mdl.capabilities/v1` 清单新增声明：
- `stop` 命令（含参数）
- 完整 `error_codes` 契约（code + http_status + 说明）

agent 可在运行时发现 stop 能力与全部错误分类。

## 实测

- capabilities 返回 `execute_commands: ['list','create','info','launch','stop']` 与 7 个 error_codes
- stop 不存在的实例 → `NOT_FOUND` (404)
- stop 缺参数 → `BAD_REQUEST` (400)
- 未知命令 → `UNKNOWN_COMMAND` (400)
- 70 项单元测试全绿

## Files

- `mdl.exe` (Windows x64 release build, v26.1.0-alpha.2)
- See `CHANGELOG.md` for the detailed changelog entry.
