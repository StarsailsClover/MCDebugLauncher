# MDL Roadmap

MCDebugLauncher 后续规划。版本命名沿用 Aprism 家族方案：每年一条主线（v26 = 2026），
每条主线含若干 minor（v26.0、v26.1 …），每个 minor 走 Alpha 预发布后收敛为正式版 Release。

> 状态图例：✅ 已完成 · 🔄 进行中 · 📋 规划中

## v26.0（已完成主线，收尾于 Alpha 12）

- ✅ Alpha 1–8.1：核心启动器、实例/模组管理、Agent 游戏控制（Despotes）、整合包导入、JE 服务端、启动更新摘要。
- ✅ Alpha 9：instance-info 命令、多语言、启动 changelog 精简。
- ✅ Alpha 10：Aprism 产品矩阵（loader / AprismRefract / AprismPrismate）+ Despotes vanilla。
- ✅ Alpha 11：真实下载进度条（单流 / 分块 / 资产批量）。
- ✅ Alpha 12：环境健康自检（`mdl doctor`）+ 本路线图。

## v26.1（下一主线）

| Alpha | 主题 | 内容 |
|---|---|---|
| Alpha 1 | 更好的 AI 集成 | 结构化 Agent 命令面、面向 LLM 的稳定 JSON 契约、能力清单查询端点 |
| Alpha 2 | 更好的 Agent 使用体验 | 减少交互确认、幂等命令、更清晰的错误码、ready 事件增强 |
| Alpha 3 | BE 启动更多支持 | 在既有 BE 专用服基础上扩展启动/注入能力（配合 injector 基建） |
| Alpha 4 | 功能追平 | 对齐主流启动器的常见能力差距（以研究结论为准） |
| Alpha 5 | 稳定性与收敛 | 回归固化、文档、正式版发布 |

## 长期候选方向（未排期）

- 正版账号皮肤渲染、多账号会话管理增强。
- 测试世界 / 测试服务器自动化体验完善。
- Aprism BE Native 加载器的启动器侧适配。
- 更多镜像与下载源的健康度持续监测。
