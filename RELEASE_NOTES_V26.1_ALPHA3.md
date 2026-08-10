# MDL v26.1 Alpha 3 — More Bedrock Launch Support

## Theme

v26.1 主线第三个 Alpha：把 Bedrock 专用服（BDS）的管理能力补齐到与 JE 专用服对等。

## 1. BDS 全生命周期管理

此前 BDS 只有 install / launch。现在补齐：

- `mdl bedrock stop <instance>` — 杀死 BDS 进程树
- `mdl bedrock status <instance> [--format json]` — 报告 installed / running / PID

## 2. 启动行为增强

- **EULA 自动接受**：BDS 首次启动写入 `eula.txt`，避免直接退出
- **日志捕获**：BDS stdout/stderr 写入 `bedrock_server.log`
- **PID 追踪**：PID 记录到 `bedrock/server/runtime/pid` 并校验存活
- **防重复启动**：已在运行的 BDS 再次 launch 返回明确错误

## 3. 修复：BDS launch 管道挂起

修复了 detached BDS spawn 的控制台句柄继承标志（与 JE server、游戏启动器同款修复），
调用 shell 立即返回而不是阻塞在管道上。

## 实测

在临时实例上完整走通 BDS 生命周期：install -> launch（秒返回，PID 记录）->
status（running）-> stop -> status（stopped）。测试实例已清理。70 项单元测试全绿。

## Files

- `mdl.exe` (Windows x64 release build, v26.1.0-alpha.3)
- See `CHANGELOG.md` for the detailed changelog entry.
