# Git 状态审查 - 2026-08-10

## 当前分支结构

### 主要分支
- **main** (ce3a7c7) - 仅包含 README 更新，落后于开发分支
- **feature/despotes-integration** (dfe4fbe, HEAD) - 最新的开发分支，包含 Alpha 6-8.1 的所有功能
- **bugfix/neoforge-classpath-and-features** (6ee1971) - 已合并，与 Alpha 6 tag 相同

### 发布分支
- **release/v26.0-alpha.2** (3d92ebf) - Alpha 3 发布
- **release/v26.0-alpha.4** (3128979) - Alpha 4 发布

---

## 🔴 问题分析

### 1. **main 分支严重落后**
**现象：**
- main 分支停留在 Alpha 5 时期（ce3a7c7）
- feature/despotes-integration 领先 main **26 个提交**
- main 分支只有 3 个 README 更新提交，缺少所有 Alpha 6-8.1 的功能

**影响：**
- 用户克隆仓库后默认得到过时版本
- GitHub 主页展示的是旧版本文档
- 发布标签（v26.0-alpha.6, v26.0-alpha.7, v26.0-alpha.8, v26.0-alpha.8.1）都在 feature 分支上

### 2. **分支混乱**
**现象：**
- 开发工作在 feature/despotes-integration 进行
- bugfix/neoforge-classpath-and-features 已合并但分支未删除
- release 分支只创建到 alpha.4，但已发布到 alpha.8.1

**问题：**
- 不清楚哪个分支是"真正的主线"
- release 分支策略不一致

### 3. **.gitignore 状态良好**
**检查结果：** ✅ 正常
- 所有构建产物、日志文件、测试文件都被正确忽略
- release/ 目录被忽略（包含 9 个历史版本压缩包）
- 开发工具文件（.claude/, SOUL.md, USER.md）被正确忽略
- 无需添加新的忽略规则

---

## 📊 提交历史对比

### feature/despotes-integration 相比 main 的新增提交（26个）
```
dfe4fbe - Alpha 8.1: modpack import, JE servers, digest fix
3f5cccf - mrpack roundtrip tests
5d5fae3 - Detached spawn fix + JE server lifecycle
988bb26 - Alpha 8.1 feature complete
27855c9 - Agent launch background task
3e3cf15 - File integrity verification
8868e50 - Optional launch queue
e4346d8 - Auto Fabric API install
b35419d - UTF-8 console fix
c200c6b - Clean up companion sources
ae63ac6 - No-proxy HTTP client
4375333 - Self-update fallback
5782ac3 - Alpha 8 release
e14d830 - Alpha 7+8 features
cd63fab - Bedrock + Aprism (Alpha 7)
4c5fda4 - Search/install + MS login (Alpha 7)
851c2d9 - Mirror/cache/i18n (Alpha 7)
6b5a2d9 - Alpha 7 release
9bea388 - Despotes selection
8ab6390 - Despotes wiring
70d20ae - Despotes integration core
6ee1971 - Alpha 6 release
1781f09 - Agent companion mod
f9f618d - Game CLI + detached launch
1c705d7 - Agent REST endpoints
fee874f - Game control module
```

### main 分支独有提交（3个 README 更新）
```
ce3a7c7 - Update README.md
28c0a1c - Update README_CN.md
f4222c7 - Update README.md
610ce2d - Merge PR #5 (NeoForge bugfix)
```

---

## 🎯 推荐方案

### 方案 A：将 feature/despotes-integration 合并回 main（推荐）

**步骤：**
1. 切换到 main 分支
2. 合并 feature/despotes-integration
3. 推送到 origin/main
4. 删除不再需要的 feature 分支（可选）

**优点：**
- main 重新成为最新代码的主线
- 符合传统 Git 工作流
- GitHub 默认分支展示最新版本

**风险：**
- 需要解决可能的 README 冲突

### 方案 B：将 main 重命名为 legacy，feature/despotes-integration 改名为 main

**步骤：**
1. 重命名 main -> legacy
2. 重命名 feature/despotes-integration -> main
3. 更新 GitHub 默认分支设置

**优点：**
- 保留历史，避免 force push
- 清晰标识旧版本

**缺点：**
- 需要修改 GitHub 设置
- 协作者需要更新本地分支

---

## 🧹 清理建议

### 立即清理
1. **合并 feature/despotes-integration 到 main**
2. **删除已合并的分支**
   - bugfix/neoforge-classpath-and-features（本地和远程）
3. **统一 release 分支策略**
   - 删除过时的 release/v26.0-alpha.2 和 release/v26.0-alpha.4
   - 或者为 Alpha 6-8.1 补充 release 分支

### 可选清理
4. **清理 release 目录**
   - 删除本地 release/*.tar.gz 和 *.zip（已在 .gitignore 中）
   - 这些应该只保留在 GitHub Releases 中
5. **添加分支保护规则**
   - 保护 main 分支，要求 PR 审查
   - 禁止直接推送到 main

---

## 📋 执行清单（推荐方案 A）

```bash
# 1. 确保所有改动已提交
git status

# 2. 切换到 main 分支
git checkout main

# 3. 拉取最新的远程更改
git pull origin main

# 4. 合并 feature/despotes-integration（可能需要解决冲突）
git merge feature/despotes-integration

# 5. 推送到远程
git push origin main

# 6. 删除已合并的本地分支
git branch -d bugfix/neoforge-classpath-and-features

# 7. 删除远程分支
git push origin --delete bugfix/neoforge-classpath-and-features

# 8. 可选：删除 feature/despotes-integration（如果不再需要）
# git branch -d feature/despotes-integration
# git push origin --delete feature/despotes-integration

# 9. 清理过时的 release 分支
git branch -d release/v26.0-alpha.2
git branch -d release/v26.0-alpha.4
git push origin --delete release/v26.0-alpha.2
```

---

## ⚠️ 注意事项

1. **合并前备份**
   - 确保所有重要工作已推送到远程
   - 考虑创建备份分支

2. **README 冲突**
   - main 有 3 个 README 更新
   - feature/despotes-integration 有大量 README 改动
   - 合并时优先使用 feature 分支的版本（更完整）

3. **标签位置**
   - 所有 Alpha 6-8.1 的标签都在 feature 分支上
   - 合并后这些标签会自然关联到 main 分支的历史

4. **协作者通知**
   - 如果有其他开发者，通知他们分支结构变化
   - 提供更新本地仓库的指令

---

## 当前未跟踪文件

```
ALPHA9_PLANNING.md       - 新创建的规划文档
FACT.md                  - 新创建的测试总结
docs/CLI_DESIGN_REVIEW.md - 新创建的设计评审
```

**建议：** 这些文档应该提交到代码库中，作为项目文档的一部分。

---

## 总结

**核心问题：** main 分支严重落后，所有最新开发都在 feature 分支

**推荐操作：** 
1. 立即将 feature/despotes-integration 合并回 main
2. 删除已合并的 bugfix 分支
3. 提交新创建的文档文件
4. 清理过时的 release 分支

**优先级：** 🔴 高 - 应在 Alpha 9 开发开始前完成
