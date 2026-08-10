# MDL v26.1.0 — Official Release

## v26.1 Official Release

第二个正式版本。将 v26.1 全主线（Alpha 1–5）折叠为一个稳定的、非预发布构建。

## 主线亮点

| Alpha | 主题 | 要点 |
|---|---|---|
| Alpha 1 | 更好的 AI 集成 | 机器可读能力清单（`/api/v1/capabilities` + `mdl capabilities`，schema `mdl.capabilities/v1`）；agent launch 支持 `java-path`/`memory`/`aprism`/`enter-test-world`；机器可读命令输出纯净 stdout |
| Alpha 2 | 更好的 Agent 体验 | agent `stop` 命令；每个 execute 失败携带机器可读 `error_code` + 匹配的 HTTP 状态 |
| Alpha 3 | 更多 BE 启动支持 | BDS 全生命周期（stop/status/EULA/日志捕获/PID 追踪/防重复启动）+ detached spawn 管道挂起修复 |
| Alpha 4 | 功能追平 | 实例 `clone` 与 `rename` |
| Alpha 5 | 稳定性 | 0 编译警告；修复死赋值、snake_case serde 字段、must-use Result、弃用 image API、两处类型可见性问题、三处重复 Win32 FFI 声明 |

## 验证

- 74 项单元测试全绿
- release 构建干净（0 警告）
- `mdl doctor` 8 passed / 0 failed
- `mdl capabilities` 返回完整能力清单

## Files

- `mdl.exe` (Windows x64 release build, v26.1.0)
- See `CHANGELOG.md` for the detailed changelog entry.
