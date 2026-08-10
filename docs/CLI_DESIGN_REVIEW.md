# CLI 命令设计审查报告

**日期**: 2026-08-09  
**版本**: Alpha 8  
**审查者**: Kiro (AI Agent)

## 执行摘要

本次审查对 MCDebugLauncher 的 CLI 命令设计进行了全面分析。总体而言，命令结构清晰、功能完善，但存在一些重叠、冗余和用户体验问题需要优化。

## 一、现有命令结构分析

### 1.1 顶层命令概览

```
核心实例管理:
- create          创建实例
- list            列出实例
- launch          启动实例
- delete          删除实例
- status          查看实例状态

实例操作:
- diagnose        诊断问题
- logs            查看日志

版本管理:
- versions        列出版本
- version-info    版本详情

功能模块 (子命令):
- mod             模组管理
- config          配置管理
- backup          备份管理
- game            游戏控制
- search          内容搜索
- account         账户管理
- bedrock         基岩服务器
- cache           缓存管理

工具命令:
- agent           启动代理服务器
- inject          DLL 注入
- info            系统信息
- update          自更新
- setup           PATH 配置
```

## 二、发现的问题

### 2.1 命令重叠和冗余

#### 问题 1: `status` 命令功能不明确
- **当前状况**: `mdl status [instance]` 显示实例运行状态（PID、内存、CPU）
- **重叠**: `mdl list` 也显示实例列表，但没有运行时信息
- **建议**: 
  - 将 `status` 合并到 `list --running` 或 `list --verbose`
  - 或保留 `status`，但增强 `list` 的输出选项

#### 问题 2: `version-info` 和 `versions` 命令分离
- **当前状况**: 两个独立的顶层命令
- **问题**: 功能相关但分离，不符合子命令模式
- **建议**: 
  - 合并为 `mdl version list` 和 `mdl version info <id>`
  - 或保持现状但在帮助文本中明确关系

#### 问题 3: `diagnose` 和 `logs` 功能重叠
- **当前状况**: 
  - `diagnose` 分析崩溃和错误
  - `logs` 查看日志文件
- **重叠**: `diagnose --analyze` 也会读取日志
- **建议**: 保持现状，但在文档中明确区分场景

### 2.2 命令参数设计问题

#### 问题 4: `launch` 命令参数过多
- **当前状况**: `launch` 有 15+ 个参数
- **问题**: 参数过多，难以记忆和使用
- **常用参数**: `--username`, `--memory`, `--detach`
- **高级参数**: `--agent`, `--aprism`, `--enter-test-world`, `--wait-ready`
- **建议**:
  - 将高级参数移到配置文件或环境变量
  - 提供常用场景的预设配置（如 `--profile dev`）

#### 问题 5: `create` 命令的 `--no-despotes` 参数
- **问题**: 双重否定（no-no）逻辑不清晰
- **当前**: `--no-despotes` 跳过安装
- **建议**: 改为 `--skip-despotes` 或 `--despotes=false`

#### 问题 6: `--format` 全局参数未充分利用
- **当前状况**: 所有命令都支持 `--format json`
- **问题**: 部分命令的 JSON 输出不完整或不一致
- **建议**: 统一 JSON 输出格式，确保所有命令都有完整的结构化输出

### 2.3 缺失的功能

#### 问题 7: 没有 `instance` 子命令组
- **当前状况**: `create`, `list`, `delete`, `status` 都是顶层命令
- **问题**: 命名空间混乱，顶层命令过多
- **建议**: 考虑创建 `instance` 子命令组：
  ```
  mdl instance create <name>
  mdl instance list
  mdl instance delete <name>
  mdl instance info <name>     # 合并 status
  ```
- **权衡**: 会增加命令长度，但结构更清晰

#### 问题 8: 缺少快速启动别名
- **问题**: 常用操作需要完整命令
- **建议**: 添加短别名：
  ```
  mdl run <instance>     # alias for launch
  mdl new <name>         # alias for create
  ```

### 2.4 用户体验问题

#### 问题 9: `launch` 命令默认阻塞
- **当前状况**: 启动游戏后，终端被阻塞直到游戏退出
- **问题**: 对于开发和测试场景，这不方便
- **用户反馈**: 任务 #11 - 需要解决阻塞问题
- **建议**: 
  - 改为默认 `--detach` 模式
  - 或提供 `--wait` 参数来显式等待
  - 显示"Press Ctrl+C to detach"提示

#### 问题 10: Fabric 实例缺少自动依赖安装
- **问题**: Fabric 实例创建后，用户需要手动安装 Fabric API
- **用户反馈**: 任务 #2 - 经常忘记安装导致 mod 报错
- **建议**: 创建 Fabric 实例时自动询问或安装 Fabric API

#### 问题 11: 缺少整合包导入功能
- **当前状况**: 只能手动创建实例和安装 mod
- **用户需求**: 任务 #3 - 支持 Modrinth/CurseForge 整合包
- **建议**: 添加 `mdl import <pack-file>` 或 `mdl create --from-pack <url>`

#### 问题 12: Agent 启动超时
- **问题**: Agent API 启动 Minecraft 时命令会超时
- **用户反馈**: 任务 #4
- **建议**: Agent 启动改为异步，返回任务 ID，通过 WebSocket 监听进度

#### 问题 13: 缺少文件完整性校验
- **问题**: 游戏文件损坏时不会自动修复
- **用户需求**: 任务 #6
- **建议**: `launch` 前自动校验并修复，或添加 `mdl verify <instance>` 命令

#### 问题 14: 没有服务端支持
- **当前状况**: 只支持客户端和 Bedrock 服务器
- **用户需求**: 任务 #7 - 需要 Java Edition 服务端支持
- **建议**: 添加 `mdl server create` 和 `mdl server launch`

## 三、设计优势

### 3.1 良好的设计
- ✅ 子命令结构清晰（`mod`, `config`, `backup`, `game`）
- ✅ JSON 输出支持良好（适合自动化）
- ✅ 全局参数一致（`--format`, `--verbose`, `--quiet`）
- ✅ 多语言支持（`--lang en/zh`）
- ✅ 游戏控制命令设计良好（`game` 子命令）

### 3.2 创新功能
- 🌟 Despotes 游戏控制集成
- 🌟 Agent API 和 WebSocket 事件流
- 🌟 Modrinth 内容搜索和安装
- 🌟 Microsoft 账户登录
- 🌟 Windows 窗口截图（非聚焦模式）

## 四、建议优先级

### 高优先级（Alpha 8.1 必须）
1. ✅ **修复 `launch` 阻塞问题** （任务 #11）
   - 添加更好的 detach 支持或改变默认行为
   
2. ✅ **Fabric API 自动安装** （任务 #2）
   - 创建 Fabric 实例时自动安装或提示
   
3. ✅ **Agent 启动超时修复** （任务 #4）
   - 改为异步启动，返回任务 ID

4. ✅ **文件完整性校验** （任务 #6）
   - 启动前自动校验和修复

### 中优先级（Alpha 8.1 建议）
5. **整合包支持** （任务 #3）
   - 支持 Modrinth modpack 导入

6. **服务端支持** （任务 #7）
   - 添加 Java Edition 服务端创建和启动

7. **中文乱码修复** （任务 #8）
   - 确保 UTF-8 编码正确处理

8. **版本更新提示** （任务 #9）
   - 启动时显示最近更新

### 低优先级（未来版本）
9. **命令结构重构**
   - 考虑引入 `instance` 子命令组（破坏性变更）

10. **参数简化**
    - 减少 `launch` 命令参数，引入配置预设

## 五、实际测试结果

### 5.1 基本命令测试

#### ✅ `mdl list`
- **结果**: 成功列出 42 个实例
- **输出**: 清晰、易读
- **建议**: 添加 `--filter` 参数按加载器筛选

#### ✅ `mdl info`
- **结果**: 显示系统信息和 Java 版本
- **输出**: 完整
- **建议**: 添加磁盘空间信息

#### ✅ `mdl versions --limit 5`
- **结果**: 正确显示最新 5 个版本
- **性能**: 快速（<2秒）
- **建议**: 添加缓存机制

### 5.2 待测试功能
由于没有运行中的实例，以下功能未能测试：
- `mdl launch` 的实际启动流程
- `mdl game` 游戏控制功能
- `mdl agent` 服务器功能
- `mdl backup` 备份恢复

## 六、具体改进建议

### 6.1 命令别名（便利性）
```bash
# 添加短别名
mdl run <instance>          # = mdl launch
mdl new <name>              # = mdl create
mdl rm <name>               # = mdl delete
mdl ls                      # = mdl list
```

### 6.2 改进 `launch` 命令
```bash
# 当前问题：默认阻塞，参数过多

# 建议的改进：
mdl launch <instance>                 # 默认 detach，显示 PID
mdl launch <instance> --wait          # 显式等待退出
mdl launch <instance> --profile dev   # 使用预设配置
mdl launch <instance> --quick         # 快速启动（跳过检查）
```

### 6.3 增强 `create` 命令
```bash
# 当前：
mdl create test --mc-version 1.21.1 --loader fabric

# 建议添加：
mdl create test --from-pack https://modrinth.com/modpack/...
mdl create test --template vanilla-dev    # 使用模板
mdl create test --clone existing-instance # 克隆现有实例
mdl create test --with-fabric-api         # 自动安装 Fabric API
```

### 6.4 新增 `verify` 命令
```bash
mdl verify <instance>              # 校验文件完整性
mdl verify <instance> --repair     # 自动修复
mdl verify <instance> --deep       # 深度校验（包括 mods）
```

### 6.5 服务端支持
```bash
mdl server create <name> --type vanilla|paper|spigot|fabric
mdl server launch <name>
mdl server stop <name>
mdl server console <name>     # 附加到控制台
```

## 七、结论

MCDebugLauncher 的 CLI 设计整体上是**成功**的，具有清晰的命令结构和强大的功能。主要改进方向应集中在：

1. **解决用户体验痛点**（launch 阻塞、Fabric API 缺失）
2. **补充缺失功能**（整合包、服务端、文件校验）
3. **优化命令参数**（减少复杂性，增加便利性）
4. **保持向后兼容**（避免破坏性变更）

建议在 Alpha 8.1 中优先实现高优先级改进，中低优先级改进可以逐步迭代。

---

**审查状态**: ✅ 完成  
**下一步**: 开始实施 Alpha 8.1 改进计划
