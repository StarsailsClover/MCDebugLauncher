# MCDebugLauncher v26.0 Alpha 8 - Release Summary

## 发布日期
2026-08-09

## 版本信息
- **版本号**: v26.0.0-alpha.8
- **分支**: feature/despotes-integration
- **测试状态**: ✅ 48/48 tests + 端到端实测（镜像/切片/缓存/检索/BDS/就绪广播）

---

## ✨ Alpha 7 剩余全部 + Alpha 8 游戏启动完毕广播

### 1. 国内镜像测活与最优线路（Alpha 7）
- 内置 BMCLAPI 国内镜像 + Mojang 官方源；启动时并发测活、按延迟排序、缓存 10 分钟
- 所有下载（版本/库/assets/模组/BDS）自动走最优线路，失败顺序回退（灵活源切换）
- URL 映射遵循 OpenBMCLAPI 约定（root / maven / assets）

### 2. 切片下载（Alpha 7）
- 大文件（≥4MB 且服务器支持 Range）切 4 片并发下载后按序拼装
- 不支持 Range 或 HEAD 被拒时自动退化为单流 + Range 探测总大小
- 下载客户端独立长超时（600s），修复大文件（BDS ~95MB）此前 30s 超时失败

### 3. 缓存制（Alpha 7，7 天副本安装）
- 任何下载物存一份源文件于 `<缓存>/`，实例安装**副本**；同一适用实例不重复下载
- 元数据记录 fetched/last_used；`mdl cache info` 查看、`mdl cache clean --days 7` 过期清理（默认 7 天）

### 4. 模组/资源包/光影检索与选择下载（Alpha 7）
- `mdl search mod|resourcepack|shader <query>`（Modrinth，project_type 区分；光影=shader、资源包=resourcepack 已实测）
- 编号列表展示；`--instance` 时输入序号安装到 mods/ / resourcepacks/ / shaderpacks/

### 5. 正版账号登录与皮肤（Alpha 7）
- `mdl account login`：Microsoft OAuth Device Code flow（无浏览器友好：打印 code+链接，用户任意设备授权）
- 完整 Xbox→XSTS→Minecraft 链路；token/uuid/皮肤 缓存于 `<数据>/accounts/`
- `mdl account list`、`mdl account skin <账号> -o out.png`（sessionserver textures + mc-heads 头像 URL）

### 6. 测试世界与自动进入（Alpha 7）
- `mdl create --with-test-world` 标记实例
- `mdl launch --enter-test-world --wait-ready`：游戏广播就绪后经 Despotes 导航进入/创建测试世界

### 7. JDK 自定义与动态内存/性能（Alpha 7）
- `--java-path` 覆盖自动检测；`--memory 4G` 显式或默认动态（系统内存一半，封顶 8G）
- 动态性能调优：≥4G 用 G1GC（MaxGCPauseMillis=50），否则 SerialGC

### 8. Aprism JE Native 加载器（Alpha 7）
- `mdl launch --aprism`：从 GitHub Releases 检测适用资产（stable 优先、pre 兜底），下载缓存并以
  `-javaagent:<jar>=aprismVersion=...;mcEdit=JE;mcVersion=...;gameRoot=...` 挂载

### 9. Minecraft BE 支持（Alpha 7）
- `mdl bedrock install <实例>`：下载官方 Bedrock 专用服（版本探测，实测 1.26.43.1，94.9MB 切片下载成功）
- `mdl bedrock launch <实例>`：空 stdio 启动 bedrock_server.exe，秒返回
- BE 客户端为 UWP 锁定，客户端侧支持以注入器 + Aprism BE 为后续

### 10. dll/exe 注入器（Alpha 7，Aprism BE 预置）
- `mdl inject <pid|进程名> --dll <路径>`（CreateRemoteThread/LoadLibraryW），本地显式操作

### 11. 日志增强（Alpha 7）
- 持久化 `<数据>/logs/mdl.log`（stdout+文件双写）、`--log-file` 覆盖、`--lang zh` 中文启动器消息

### 12. Alpha 8：游戏启动完毕广播
- 启动后 agent 服务器轮询 Despotes，游戏就绪（inGame 或 screenOpen）即广播 `game_ready` 事件（WebSocket/JSON）
- `GET /api/v1/game/<实例>/ready` 返回 ready 状态（503 未就绪）
- `mdl launch --wait-ready` 阻塞至就绪（实测 ready:true）

---

## 🧪 测试验证
- 单元/集成 48/48 通过；镜像映射、缓存副本、切片拼装、检索、注入、BDS URL 均有测试
- 端到端：BDS 下载+启动、Despotes 就绪广播（ready:true）、镜像回退下载

## 📝 使用须知
- 镜像测活在网络差时自动落回官方源；缓存清理不影响已安装实例（副本独立）
- 正版登录需用户在任意设备完成 device code 授权；token 过期需重登

---

**版本**: v26.0.0-alpha.8
**发布日期**: 2026-08-09
