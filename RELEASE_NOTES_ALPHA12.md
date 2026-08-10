# MDL v26.0 Alpha 12 — Release Notes

## 核心内容

Alpha 12 是 v26.0 主线的收尾版本，引入两项研究与规划成果：

### 1. 环境健康自检（`mdl doctor`）

新增 `mdl doctor` 命令，对 MDL 运行环境进行只读全面体检：

- **Java 运行时**：检测版本与路径
- **目录布局**：数据/缓存/实例等 7 个关键目录是否存在
- **下载缓存**：条目数与占用空间
- **镜像源延迟**：探测所有镜像源，报告最快源与延迟
- **实例状态**：磁盘上的实例数量
- **外部服务可达性**：Mojang（版本清单）、Modrinth（模组搜索）、GitHub（Despotes/Aprism 资产）

每项以 `[OK]` / `[WARN]` / `[FAIL]` 标注并附详情行；存在硬失败时命令以非零退出码结束。

### 2. 版本路线图（ROADMAP.md）

新增 `ROADMAP.md` 文档，记录：
- v26.0 主线完成状态（Alpha 1–12）
- v26.1 下一主线的规划主题（AI 集成、Agent 体验、BE 支持、功能追平、稳定性收敛）
- 长期候选方向（未排期）

## 实测结果

```
MDL environment health check
============================
[OK]   java           Java 21.0.11 (major 21) at java
[WARN] directories    1 directory(ies) not yet created (auto-created on first use): assets
[OK]   cache          0 entries, 0.00 MB
[OK]   mirrors        2/2 reachable, fastest: bmclapi (211 ms)
[OK]   instances      55 instance(s) on disk
[OK]   mojang         version manifest reachable (268 ms)
[OK]   modrinth       search API reachable (499 ms)
[OK]   github         GitHub API reachable (572 ms)

Result: 8 passed, 0 failed
```

## 验证

- 67 项单元测试全部通过（+3 项 opt-in 网络集成测试）
- release 构建无错误
- `mdl doctor` 端到端实测：8 passed, 0 failed
