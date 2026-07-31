# MCDebugLauncher v26.0 Alpha 5 - Release Summary

## 发布日期
2026-07-31

## 版本信息
- **版本号**: v26.0.0-alpha.5
- **二进制大小**: 9.2 MB
- **分支**: bugfix/neoforge-classpath-and-features
- **提交数**: 3 commits
- **测试状态**: ✅ 全部通过 (16/16 tests)

---

## 🔧 关键修复

### 1. NeoForge启动致命错误修复

**问题描述**:
```
Fatal Startup Error: Failed to create entrypoint object
net.neoforged.fml.startup.FatalStartupException: 
Missing main class net.minecraft.client.main.Main from the game content loader
```

**根本原因**:
- NeoForge依赖installer生成的patched client JAR（反混淆+二进制补丁）
- MDL对NeoForge实例跳过vanilla client JAR
- 如果patched client缺失或损坏，Minecraft核心类不可用
- 多模组环境下更容易触发此问题

**解决方案**:
- 在`build_classpath()`函数中添加fallback逻辑
- 检测到patched client缺失时，自动添加vanilla client JAR到classpath
- 确保`net.minecraft.client.main.Main`等核心类始终可用
- 记录WARN级别日志便于调试

**代码位置**: `src/instance/launcher.rs` 行514-524

---

## ✨ 新功能

### 2. 自动更新系统

**命令**:
```bash
mdl update --check    # 仅检查更新
mdl update            # 交互式更新
```

**功能特性**:
- ✅ GitHub API集成，自动检查最新release
- ✅ 语义化版本比较（支持alpha/beta标签）
- ✅ 自动下载新版本二进制
- ✅ 更新前自动创建备份（.exe.bak）
- ✅ Windows平台生成批处理脚本安全替换
- ✅ 网络失败时提供手动下载链接

**实现文件**: `src/util/selfupdate.rs` (192行)

### 3. 环境变量注册

**命令**:
```bash
mdl setup    # 将MDL添加到PATH
```

**功能特性**:
- ✅ 自动添加MDL到系统PATH（Windows）
- ✅ PowerShell集成修改环境变量
- ✅ 重复条目检测（不会重复添加）
- ✅ 设置时自动检查更新
- ✅ 跨平台感知（非Windows提示手动配置）

**使用场景**:
运行一次`mdl setup`后，可在任何终端窗口直接使用`mdl`命令，无需完整路径。

### 4. 窗口标题显示模组（已有功能验证）

**功能说明**:
- 终端窗口标题: `MDL: <实例名> [模组1, 模组2, ...]`
- 游戏窗口标题: 同样格式（通过`--title`参数）

**示例**:
```
Loaded 2 mod(s) in 'bc-test':
  - Fabric API (0.119.4+1.21.4)
  - MinecraftBC (2.0.0)

[窗口标题]: MDL: bc-test [Fabric API, MinecraftBC]
```

**用途**: 方便在多实例测试时快速识别窗口对应的实例和模组环境。

---

## 📊 技术统计

### 代码变更
- **修改文件**: 4个
- **新增行数**: 274行
- **新增模块**: `src/util/selfupdate.rs`

### 受影响组件
- `src/instance/launcher.rs` - NeoForge classpath逻辑
- `src/main.rs` - 新增update和setup命令
- `src/util/mod.rs` - 注册selfupdate模块
- `src/util/selfupdate.rs` - 完整的自更新实现

### 依赖项
无新增依赖，使用现有：
- `reqwest` - HTTP请求
- `tokio::fs` - 异步文件操作
- `serde_json` - GitHub API解析
- `anyhow` - 错误处理

---

## 🧪 测试验证

### 自动化测试
```
running 16 tests
✅ All tests passed
```

### 手动测试
- ✅ NeoForge 1.21.1实例启动正常
- ✅ Update命令版本检查工作正常
- ✅ Setup命令成功添加PATH
- ✅ 模组列表正确显示在窗口标题
- ✅ Fabric/Forge实例不受影响

---

## 📦 发布资产

### 发布包内容
```
release/mdl-v26.0-alpha.5-windows-x64/
├── mdl.exe                (9.2 MB)
├── README.md              (4.2 KB)
├── README_CN.md           (3.7 KB)
├── CHANGELOG.md           (3.9 KB)
├── LICENSE                (620 B)
└── BUGFIXES_ALPHA5.md     (5.6 KB)
```

### Git信息
- **分支**: bugfix/neoforge-classpath-and-features
- **基于**: v26.0-alpha.4 (tag: v26.0-alpha.4)
- **提交**:
  1. `d49635e` - Fix NeoForge classpath bug and add self-update features
  2. `4452d19` - Bump version to 26.0.0-alpha.5
  3. `84f2536` - Add detailed documentation for Alpha 5 fixes

---

## 📝 用户迁移指南

### 对现有用户
- ✅ **无破坏性变更**
- ✅ 现有实例无需修改即可使用
- ✅ 新命令是可选增强功能
- 💡 建议运行`mdl setup`一次以获得PATH便利性

### 升级步骤
```bash
# 方法1: 手动替换（当前Alpha 5无自动更新）
1. 下载 mdl-v26.0-alpha.5-windows-x64.zip
2. 解压并替换现有的 mdl.exe
3. 运行 mdl setup（可选）

# 方法2: 未来版本使用自动更新
mdl update
```

---

## 🔮 未来规划

### Alpha 6计划
- 性能优化（大型模组包加载速度）
- 缓存改进（减少重复下载）
- 内存使用优化

### Beta 1目标
- TUI界面（终端交互式界面）
- 或 GUI包装器（考虑中）

### 长期目标
- 跨平台二进制分发（Linux、macOS）
- 插件系统支持自定义mod loader
- Modrinth/CurseForge集成自动下载模组
- 实例模板和克隆功能

---

## 🙏 致谢

本版本修复了社区报告的关键NeoForge启动问题，并实现了用户请求的自动更新功能。感谢所有测试者的反馈。

---

## 📞 支持

- **问题报告**: GitHub Issues
- **文档**: README.md, README_CN.md
- **技术细节**: docs/BUGFIXES_ALPHA5.md

---

**版本**: v26.0.0-alpha.5  
**发布日期**: 2026-07-31  
**下一版本**: v26.0.0-alpha.6 (计划中)
