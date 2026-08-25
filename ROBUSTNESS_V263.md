# v26.3 鲁棒性评估报告（Robustness Assessment）

规划 v26.4 前对上一主线执行的模拟使用与恶意攻击测试。

- **评估对象**: mdl 26.3.0 (Windows x64 official)
- **日期**: 2026-08-25
- **重点**: v26.3 新增面——名称校验绕过、Agent API 新端点/命令、
  rotate-rcon、oom-confirm 解析

## 结果矩阵

| # | 攻击面 | 用例 | 结果 |
|---|---|---|---|
| T1a | 保留名变体 | `CON.txt.bak` / `nul.` / 含制表符 | ✅ 拦截 |
| T1b | 上标变体 | `COM¹` / `com².txt` | ❌ **绕过创建**（F1）|
| T1c | `..a` 字面前缀 | 非 traversal（字面目录名）| ✅ 安全 |
| T2 | 端点穿越 | `/instance/..%2F..%2Fetc/disk` | ✅ 404 |
| T3 | 不存在实例 metrics | | ✅ NOT_FOUND 干净 JSON |
| T4/T5 | inject-agent/server-cmd 参数缺省 | | ✅ 不崩溃，但 error_code=INTERNAL（F2）|

## 发现

### F1 [Low-Medium] 上标数字设备名绕过
Windows 同样保留 `COM¹²³`；stem 校验用 ASCII uppercase 未归一化上标。
**已修复（v26.4-alpha.1）**: 归一化 ¹²³→123 后再比对黑名单 + 回归测试。

### F2 [Low] usage 类错误误标 INTERNAL
inject-agent/server-cmd 参数缺省时 bail "Usage: ..." 被 classify 落入
INTERNAL。**已修复（v26.4-alpha.1）**: classify_error 对 `usage:` 前缀
返回 BAD_REQUEST。

### 附带性能发现（承接 alpha.6）
`mdl status` 全量路径逐实例 `System::new_all()`。
**已修复（v26.4-alpha.1）**: 单快照共享探测，status p95
1663–3517ms → **199ms**（稳态），p50 808→184ms。

*全部 rb-/临时实例已清理。*
