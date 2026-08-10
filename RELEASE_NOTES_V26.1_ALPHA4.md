# MDL v26.1 Alpha 4 — Feature Parity (Instance Clone & Rename)

## Theme

v26.1 主线第四个 Alpha：补齐主流启动器都有而 MDL 缺失的实例管理能力。

## 1. 实例克隆（clone）

`mdl clone <src> <dst>`

- 递归复制整个实例目录树（mods、configs、saves、worlds）
- 新实例的 `instance.json` 自动改写为新名字
- 目标已存在时返回明确错误

## 2. 实例重命名（rename）

`mdl rename <old> <new>`

- 重命名实例目录并同步更新配置
- 目标已存在时返回明确错误

## 实测

- 74 项单元测试全绿（新增 4 项 clone/rename 测试：递归复制、配置改写、目标冲突拒绝）

## Files

- `mdl.exe` (Windows x64 release build, v26.1.0-alpha.4)
- See `CHANGELOG.md` for the detailed changelog entry.
