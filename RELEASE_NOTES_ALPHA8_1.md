# MCDebugLauncher v26.0 Alpha 8.1 - Release Summary

## 发布日期
2026-08-10

## 版本信息
- **版本号**: v26.0.0-alpha.8.1
- **分支**: feature/despotes-integration
- **测试状态**: ✅ 58/58 tests + 端到端实测（服务端全生命周期、更新纲要、整合包往返测试、detach 秒返回）

## 本次变更

### 新增功能

#### 1. Modrinth 整合包导入（自动补全）
- `mdl import <name> <pack.mrpack>` 解析 `modrinth.index.json`，按整合包声明的精确 Minecraft 版本与加载器创建实例
- 自动复制 `overrides/`（含 `client-overrides/`）到实例目录
- 自动补全所有索引文件：sha1 校验下载，文件已存在且完好则跳过（幂等，可重复执行）
- 防 zip-slip：拒绝绝对路径与 `..` 逃逸路径
- `--no-download` 仅导入结构与 overrides，不下载文件

#### 2. JE 服务端支持
- `mdl server create <name> --mc-version <ver> [--memory 4G]`：从版本清单下载官方 server.jar（sha1 校验），自动写入 `eula.txt` 与默认 `server.properties`
- `mdl server launch <name> [--attach]`：默认后台运行（detach），PID 跟踪、`server.log` 记录；`--attach` 前台阻塞
- `mdl server stop <name>`：终止运行中的服务端
- `mdl server status <name>` / `mdl server list`：查看状态，支持 `--format json`
- 服务端存放在 `<data>/servers/<name>/`

#### 3. 启动时更新纲要
- 每次运行 MDL 时打印最近 4 个版本的更新摘要（编译期嵌入 CHANGELOG 解析）
- `--format json` 时自动隐藏，不影响机器可读输出

### 修复

#### 4. Detached 启动管道句柄泄漏（重要体验修复）
- Windows 上 detach 子进程（游戏与服务器）继承了 stdout/stderr 句柄，导致 `mdl launch --detach` 与 `mdl server launch` 在 shell 中表现为挂起（实际要等子进程退出管道才关闭）
- 修复：spawn 前清除标准流句柄的继承标志 + 服务端 detach 使用 `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`
- 同时惠及既有的客户端 `--detach` 路径
- 实测：`mdl server launch` 从挂起 60s+ 降为 ~1s 返回

### 变更
- Fabric 实例的启动时 Fabric API 修复（Alpha 8 引入）在 8.1 端到端验证通过
- README 重写，反映 Alpha 5–8.1 完整能力

## 11 项需求完成状态
| # | 需求 | 状态 |
|---|------|------|
| 1 | Fabric API 自动补装 | ✅ 已实现并验证 |
| 2 | 整合包自动补全 | ✅ 本次实现 |
| 3 | Agent 启动超时 | ✅ 已实现（后台任务+事件流） |
| 4 | 性能/体验优化 | ✅ detach 秒返回修复 + 分块下载/缓存（Alpha 8） |
| 5 | 启动前校验自补全 | ✅ 已实现并验证 |
| 6 | 服务端创建与启动 | ✅ 本次实现 |
| 7 | 中文乱码 | ✅ 已实现（UTF-8 控制台） |
| 8 | 启动更新纲要 | ✅ 本次实现 |
| 9 | 文档更新 | ✅ 本次更新 README/CHANGELOG |
| 10 | launch 阻塞 | ✅ 已实现（--detach）+ 句柄泄漏修复 |
| 11 | 排队可选 | ✅ 已实现（--no-queue） |

## 发布资产
- Windows x64 二进制（release profile: LTO + strip）
- 源码 tag: v26.0-alpha.8.1
