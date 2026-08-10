# MDL v26.1 Alpha 5 — Stability & Code Hygiene

## Theme

v26.1 主线第五个（最后一个）Alpha：稳定性与代码卫生。目标是把构建做到 **0 警告**，
并修复其中暴露的真实问题。

## 修复的真实问题

- **死赋值**：`is_neoforge` 初始值从不被读取，移除
- **命名规范**：Xbox API 响应字段改为 snake_case + `#[serde(rename)]`（保留 PascalCase 线上键）
- **must-use Result**：`capture.stop()` 的 Result 显式处理
- **弃用 API**：`image::io::Limits` → `image::Limits`
- **API 设计**：修复两处"type more private than item"（`FabricLoaderProfile`、`Rule` 提升为 `pub`）
- **未使用导入**：移除 `Context`、两处 `std::io::Write`
- **FFI 一致性**：三个模块重复声明的 Win32 符号（`OpenProcess`/`CloseHandle`/`GetStdHandle`）
  统一为单一指针句柄签名，消除三处"redeclared with a different signature"警告

## 有意保留的表面

对 serde 线上格式字段与预留工具函数的 dead_code，改用一条带文档的 crate 级 allow，
而非逐项标注，明确这是有意的 API 表面而非 bug。

## 验证

- 74 项单元测试全绿
- release 构建 0 警告
- `mdl doctor` 8 passed / 0 failed
- `mdl capabilities` 返回完整能力清单

## Files

- `mdl.exe` (Windows x64 release build, v26.1.0-alpha.5)
- See `CHANGELOG.md` for the detailed changelog entry.
