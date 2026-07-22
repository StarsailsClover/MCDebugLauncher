# MCDebugLauncher 研究文档

## 项目概述

MCDebugLauncher (MDL) 是一个命令行 Minecraft 启动器，专为开发者、测试人员和 AI 代理设计。它支持快速测试任意 Mod 加载器（包括 OptiFine）、自动化 Mod 安装、全面的错误日志记录，以及专门的开发者/代理功能。

## 核心需求

1. **多加载器支持**: Vanilla、Forge、NeoForge、Fabric、Quilt、LegacyFabric、OptiFine
2. **快速测试**: 单条命令完成任意版本+加载器组合的安装和启动
3. **错误诊断**: 自动收集并导出崩溃报告、日志和诊断数据
4. **代理友好**: 结构化输出格式（JSON）以便程序化控制
5. **开发者工具**: 调试日志、性能分析、无头模式支持
6. **企业级规范**: 生产级代码质量、全面的错误处理

## 技术调研总结

### 1. Minecraft 版本管理

**官方 API (Mojang/Microsoft)**:
- 版本清单: `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
- 返回所有可用的 Minecraft 版本及元数据
- 每个版本链接到详细的 JSON，包含:
  - 客户端/服务器下载 URL 及 SHA1 哈希
  - 资源索引 URL
  - 库依赖（平台特定）
  - Java 版本要求
  - 启动参数（JVM 和游戏）

**实现模式**:
```
1. 获取 version_manifest_v2.json
2. 解析并按版本类型筛选（release/snapshot）
3. 下载特定版本的 JSON
4. 下载 client.jar、库文件、资源文件
5. 验证 SHA1 校验和
6. 构造启动命令
```

### 2. Mod 加载器安装

#### Fabric
- **安装器 API**: `https://meta.fabricmc.net/v2/versions/loader`
- **安装方式**: 下载安装器 JAR，以 `client` 模式运行 `java -jar`
- **依赖**: 需要单独安装 Fabric API mod
- **版本格式**: `fabric:<mc_version>` 或 `fabric:<loader_version>`

#### Forge / NeoForge
- **Forge**: `https://files.minecraftforge.net/net/minecraftforge/forge/index.html`
- **NeoForge**: `https://maven.neoforged.net/releases/net/neoforged/neoforge/`
- **安装方式**: 运行安装器 JAR，在原版启动器结构中生成配置文件
- **格式**: 创建带有库和 tweaker 的版本 JSON
- **注意**: NeoForge 是现代分支（1.20.2+），Forge 用于旧版本

#### Quilt
- **安装器**: `https://quiltmc.org/install/`
- **类似 Fabric**: 轻量级，需要 Quilted Fabric API
- **Fabric 兼容**: 可以运行大多数 Fabric mod

#### OptiFine
- **无官方 API**: 必须爬取或使用镜像（如 BMCLAPI）
- **安装流程**:
  1. 下载 OptiFine 安装器 JAR
  2. 使用 `java -cp <installer> optifine.Patcher <vanilla.jar> <installer> <output>` 提取
  3. 如果捆绑，安装 launchwrapper
  4. 生成带有 tweaker 类的版本 JSON
- **兼容性**: 可独立安装或在 Forge/Fabric 之上安装（通过 OptiFabric）

### 3. 现有启动器架构

#### PrismLauncher (C++/Qt)
- **架构**: 集中式 `Application` 单例，Qt Model/View 模式
- **任务系统**: 异步任务链与进度跟踪
- **实例管理**: 隔离实例，独立配置
- **优势**: 成熟的代码库，全面的 GUI
- **局限**: GUI 中心，不为 CLI/自动化设计

#### PortableMC (Python/Rust)
- **Python 版本**: 单文件脚本，最少依赖
- **Rust 版本**: 快速、编译型、现代架构
- **特性**:
  - 单命令启动: `portablemc start <version>`
  - Mod 加载器前缀: `fabric:1.21.4`, `forge:1.20.1`
  - 用于 CI/CD 的无头 LWJGL 补丁
  - Java 运行时自动下载
- **优势**: 简洁的 CLI 设计，自动化友好
- **架构**: 模块化，版本无关的核心

#### HeadlessMC (Java)
- **用途**: CI/CD 管道中的无头 Minecraft 测试
- **特性**:
  - LWJGL 补丁实现无头模式
  - HMC-Specifics mods 用于命令行控制
  - JVM 内存启动
  - 通过命令自动化游戏（`msg`、`gui`、`click`、`connect`）
- **使用场景**: 自动化测试、mod 开发、CI 管道
- **优势**: 全面的测试能力

#### MC-CLI / Shard (Rust)
- **用途**: LLM 代理控制接口
- **特性**:
  - JSON 结构化输出以便解析
  - 命令: `status`、`teleport`、`shader`、`capture`、`analyze`
  - 多实例管理
  - 内置截图/比较工具
- **目标**: AI 代理、自动化工作流
- **架构**: 客户端-服务器模型（TCP JSON）

### 4. 日志收集和错误分析

#### 日志文件位置
- **Vanilla/Paper/Spigot**: `logs/latest.log`
- **Forge**: `logs/latest.log`、`logs/debug.log`（使用调试配置）
- **Fabric**: `logs/latest.log`、`fabricloader.log`（关键错误时）
- **崩溃报告**: `crash-reports/crash-<timestamp>.txt`
- **JVM 崩溃**: `hs_err_pid<pid>.log`

#### Log4j 配置
- **启用调试日志**: `-Dlog4j.configurationFile=<custom_log4j.xml>`
- **数据包日志**: 标记 `NETWORK_PACKETS` 用于协议调试
- **自定义日志级别**: `trace`、`debug`、`info`、`warn`、`error`

#### 崩溃报告分析
关键部分:
1. **描述**: 错误类型（如 `NoClassDefFoundError`、`NullPointerException`）
2. **堆栈跟踪**: 识别跟踪中的 mod 包名
3. **疑似 Mod**: Forge 自动识别潜在原因
4. **系统详情**: Java 版本、操作系统、内存、mod 列表
5. **Mixin 崩溃**: 查找 `mod-id$handlerName` 模式

#### 自动化诊断工具
- **Forge**: 内置 `CrashReportAnalyser`（扫描堆栈跟踪中的 mod 包）
- **Fabric**: Mixin 错误清楚地识别目标类
- **在线分析器**: mclo.gs、pastebin 解析器
- **常见模式**:
  - 缺少依赖: `NoClassDefFoundError`
  - 版本不匹配: `UnsupportedClassVersionError`
  - Mod 冲突: `ConcurrentModificationException`、Mixin 崩溃

### 5. 代理友好的设计模式

#### 结构化输出
- **JSON 格式**: 所有命令输出应可机器解析
- **退出代码**: 标准 Unix 约定（0 = 成功，非零 = 错误）
- **进度指示器**: 基于百分比或事件流格式

#### 命令接口示例
```bash
# 版本列表
mdl versions --format json --type release

# 带进度的安装
mdl install fabric:1.21.4 --progress json

# 带结构化日志的启动
mdl launch my-instance --log-format json --output logs/session.jsonl

# 诊断导出
mdl diagnose my-instance --export diagnostics.tar.gz
```

#### 代理控制模式（来自 MC-CLI/HeadlessMC）
- **状态查询**: 游戏状态、玩家位置、物品栏
- **命令**: 向运行的实例发送聊天/命令
- **捕获**: 截图、世界数据、性能指标
- **事件**: 订阅游戏事件（加入、伤害、聊天）

## 技术栈推荐

### 方案 1: Rust（推荐）
**优点**:
- 快速、内存安全、单二进制分发
- 出色的错误处理（Result/Option 类型）
- 强大的异步生态系统（tokio）
- 跨平台，无运行时依赖
- JSON 序列化（serde）
- PortableMC Rust crate 可作为参考

**缺点**:
- 学习曲线较陡
- 编译时间较长

**库**:
- `clap`: CLI 参数解析
- `serde`/`serde_json`: JSON 处理
- `reqwest`: HTTP 客户端用于 API 调用
- `tokio`: 异步运行时
- `sha1`: 校验和验证
- `zip`: 归档处理
- `log`/`tracing`: 日志基础设施

### 方案 2: Python
**优点**:
- 快速开发
- 丰富的生态系统（requests、click、rich 用于 CLI）
- 简单的 JSON/API 处理
- PortableMC 参考实现

**缺点**:
- 需要 Python 运行时
- 执行速度较慢
- 分发复杂性（PyInstaller 等）

### 方案 3: Go
**优点**:
- 快速编译，单二进制
- 良好的标准库
- 出色的并发性
- 跨平台编译支持

**缺点**:
- Minecraft 生态系统不够成熟
- 错误处理较冗长

## 提议的架构

### 核心组件

#### 1. 版本管理器
- 获取和缓存版本清单
- 下载和验证游戏文件
- 管理 Java 运行时安装
- 跟踪已安装的版本

#### 2. 加载器管理器
- 所有 mod 加载器的抽象接口
- 加载器特定的安装器（Fabric、Forge、NeoForge、Quilt、OptiFine）
- 依赖解析
- 版本兼容性检查

#### 3. 实例管理器
- 创建/删除/列出实例
- 实例配置（内存、JVM 参数、mod）
- 实例隔离（独立的 .minecraft 文件夹）
- 导入/导出能力

#### 4. 启动管理器
- 构造启动命令
- 环境变量设置
- 进程管理
- 输出捕获和解析

#### 5. 诊断管理器
- 日志收集和聚合
- 崩溃报告解析
- 自动化错误检测
- 诊断包生成

#### 6. 代理接口
- JSON-RPC 或 REST API 服务器
- 命令执行
- 事件流
- 状态查询

### 项目结构
```
MCDebugLauncher/
├── src/
│   ├── main.rs              # CLI 入口点
│   ├── version/             # 版本管理
│   │   ├── manifest.rs
│   │   ├── downloader.rs
│   │   └── java.rs
│   ├── loader/              # Mod 加载器支持
│   │   ├── fabric.rs
│   │   ├── forge.rs
│   │   ├── neoforge.rs
│   │   ├── quilt.rs
│   │   └── optifine.rs
│   ├── instance/            # 实例管理
│   │   ├── config.rs
│   │   ├── manager.rs
│   │   └── launcher.rs
│   ├── diagnostic/          # 错误分析
│   │   ├── log_parser.rs
│   │   ├── crash_analyzer.rs
│   │   └── collector.rs
│   ├── agent/               # 代理接口
│   │   ├── server.rs
│   │   ├── commands.rs
│   │   └── events.rs
│   └── util/                # 工具
│       ├── http.rs
│       ├── checksum.rs
│       └── archive.rs
├── docs/
│   ├── RESEARCH.md          # 英文研究文档
│   ├── RESEARCH_CN.md       # 本文档
│   ├── ARCHITECTURE.md      # 详细设计
│   └── API.md               # 命令参考
├── tests/
│   ├── integration/
│   └── unit/
├── Cargo.toml               # Rust 依赖
├── README.md                # 英文
├── README_CN.md             # 中文
└── LICENSE                  # Apache 2.0
```

## 实施阶段

### 阶段 1: 基础（第 1-2 周）
- [ ] 使用 Rust/Cargo 设置项目
- [ ] 使用 clap 的 CLI 框架
- [ ] 版本清单获取
- [ ] 基本原版 Minecraft 下载和启动
- [ ] 配置管理

### 阶段 2: Mod 加载器（第 3-4 周）
- [ ] Fabric 安装器实现
- [ ] Forge 安装器实现
- [ ] NeoForge 安装器实现
- [ ] Quilt 安装器实现
- [ ] OptiFine 安装器实现

### 阶段 3: 实例管理（第 5 周）
- [ ] 实例创建/删除
- [ ] 实例隔离
- [ ] Mod 文件夹管理
- [ ] 配置文件

### 阶段 4: 诊断（第 6 周）
- [ ] 日志文件收集
- [ ] 崩溃报告解析
- [ ] 错误模式检测
- [ ] 诊断包导出

### 阶段 5: 代理接口（第 7 周）
- [ ] JSON 输出格式化
- [ ] 命令 API 设计
- [ ] 事件流
- [ ] 文档

### 阶段 6: 测试和完善（第 8 周）
- [ ] 集成测试
- [ ] CI/CD 设置
- [ ] 文档完成
- [ ] 二进制打包

## 关键设计决策

### 1. 无 GUI
纯命令行界面以实现最大的自动化潜力。需要 GUI 的用户可以使用 PrismLauncher、MultiMC 等。

### 2. 标准目录结构
使用官方 Minecraft 目录结构（`.minecraft/`）以实现与现有工具和 mod 的最大兼容性。

### 3. JSON 优先输出
所有命令支持 `--format json` 用于机器解析。人类可读格式是默认值。

### 4. 隔离实例
每个实例完全独立，拥有自己的 mod、配置、存档和 Java 版本。

### 5. 主动错误处理
绝不静默失败。始终提供可操作的错误消息和建议修复方案。

### 6. 离线优先
在本地缓存一切。在初始下载后支持完全离线操作。

## 风险分析

### 技术风险
1. **OptiFine 安装**: 无官方 API，需要 JAR 内省
   - *缓解措施*: 使用 BMCLAPI 镜像，实现健壮的回退机制

2. **启动器变更**: Mojang 可能更改版本清单格式
   - *缓解措施*: 版本格式检测，向后兼容

3. **Mod 加载器更新**: 安装器可能更改其 CLI 接口
   - *缓解措施*: 版本特定的安装器，更新检测

4. **Java 兼容性**: 不同版本需要不同的 Java 运行时
   - *缓解措施*: 自动检测并从 Mojang 下载适当的 Java

### 维护风险
1. **Mod 加载器生态系统**: 可能出现新的加载器
   - *缓解措施*: 加载器支持的插件架构

2. **破坏性更改**: Minecraft 更新可能破坏假设
   - *缓解措施*: 全面的测试套件，版本特定的处理

## 参考资料

- [Minecraft 版本清单](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json)
- [PortableMC GitHub](https://github.com/mindstorm38/portablemc)
- [PrismLauncher 架构](https://github.com/PrismLauncher/PrismLauncher)
- [HeadlessMC](https://github.com/headlesshq/headlessmc)
- [MC-CLI](https://github.com/Th0rgal/mc-cli)
- [Fabric Meta API](https://meta.fabricmc.net/)
- [Forge Files](https://files.minecraftforge.net/)
- [NeoForge Maven](https://maven.neoforged.net/)
- [Wiki.vg Game Files](https://wiki.vg/Game_Files)

## 下一步

1. 确定技术选择（推荐 Rust）
2. 设置项目仓库结构
3. 实施阶段 1: 基础
4. 创建全面的 API 文档
5. 建立测试框架
6. 开始 CI/CD 管道设置
