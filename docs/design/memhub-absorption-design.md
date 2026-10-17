# MemHub / One Memory 可吸纳能力设计文档

> 状态：草稿
> 日期：2026-10-17
> 来源：腾讯混元 MemHub / One Memory 公开 README（源码版本 1.0.3，build 7；SDK `hy-memory==2.0.3`）
> 定位：对照 memrec 现状，识别 MemHub 在产品纪律、治理与宿主接入层可吸纳之处
> 关联：`docs/design/hy-memory-absorption-design.md`（记忆引擎侧能力）
> 说明：本文只做设计，不落地实现

---

## 1. MemHub 是什么

MemHub 是**产品与治理层**，不是记忆引擎本身：它把用户授权的多个 AI 产品（Claude Code、Cursor、Codex、OpenCode、Qwen Code 等）的历史对话与新增内容汇总为同一份记忆，并在独立授权后供这些 AI 复用。其记忆引擎是 Hy Memory（Python SDK）。One Memory / `one-memory` 是历史模块名、数据目录、服务与兼容命令。

关键：**Hy Memory 解决"记忆怎么存与取"，MemHub 解决"多应用如何合规地共享这一份记忆"**。memrec 目前处于两者之间的偏引擎位置（daemon + CLI + MCP），因此 MemHub 值得吸纳的是**产品纪律与治理模型**，而非记忆形态。

---

## 2. MemHub 的核心原则（原文提炼）

1. 一次安装、一个 Runtime、一份数据、多种客户端。
2. Python Core 是唯一记忆数据面与唯一业务数据库写入者。
3. 来源读取、模型处理、宿主读取是相互独立的授权边界。
4. 正式记忆链路：`Evidence -> Hy Memory -> Governance -> Matters/Wiki`。
5. 不包含屏幕、Accessibility/AXTree、OCR、键鼠、剪贴板或后台录音采集。
6. 发布有明确平台合同：只有同时出现在 GitHub Release 与 `release-manifest.json`、且通过公开校验的制品才算正式支持。
7. 数据生命周期严格：`npm uninstall` 只移除 CLI，`memhub uninstall` 保留 Memory，只有独立的 `memhub delete-data`（二次确认精确路径）才删数据。

---

## 3. memrec 现状盘点

| 方面 | memrec 载体 | 说明 |
|---|---|---|
| 安装 | `mr-install/`（`install_binaries`、`download_model`、`verify`） | 编译分发 + 模型下载 + 验证 |
| 服务管理 | `mr-install/src/systemd.rs`、`launchd.rs`、`scripts/systemd/`、`scripts/*.sh` | 已有 systemd/launchd 与启停脚本 |
| Runtime 控制 | `scripts/memrecctl.sh`、`start/stop/restart/status.sh` | 已有，但分散 |
| 契约 | `mr-protocol/`（JSON-RPC 请求/响应） | 等价于 MemHub 的 `packages/contracts` |
| 宿主接入 | `memrec/src/mcp/`、`dsh-plugin/`、`pi-extension/` | 均以 memrec 为唯一客户端入口，无需适配器族 |
| 单写入者 | 所有 CLR/MCP 均经 Unix Socket 调 `memrecd` | 事实上单写入，但未显式约束与文档化 |
| Evidence 层 | `mr-ability/src/archive/`（SQLite，按年切分，永不删除） | 等价于 MemHub 的 Evidence |
| 派生层 | `dream/phases/personal_summary.rs`（个人画像摘要） | 部分等价于 Matters/Wiki |
| 治理层 | 无 | **缺失** |
| 授权边界 | 无 | **缺失** |
| 文档 | `SECURITY_ANALYSIS.md`、`SECURITY_HIGHLIGHTS.md`、`SECURITY_SUMMARY.md`、`MANUAL.md` | 有安全，缺隐私边界与数据生命周期说明 |

---

## 4. 对照矩阵

| MemHub 能力 | memrec 现状 | 判定 |
|---|---|---|
| 一次安装 / 一 Runtime / 一份数据 / 多客户端 | 已具备（daemon + socket + 多项目多客户端） | 等价 |
| 唯一数据面与唯一写入者 | 事实具备，未约束/未文档 | **部分（可显式化）** |
| 来源/模型/宿主三级授权边界 | 无 | **缺失** |
| Evidence -> Memory -> Governance -> 派生 | Evidence + Memory 有，Governance/派生弱 | **部分** |
| 不采屏幕/键鼠/剪贴板 | memrec 仅存显式写入内容 | 天然满足，可文档化 |
| 平台支持合同 + release manifest | 有 CI/制品但无显式合同 | **缺失（可文档化）** |
| 卸载保留数据 + 独立 delete-data | 有硬删除，无守卫式 delete-data | **部分** |
| `doctor` 诊断 | 有 `verify` 与 `status`，未聚合 | **部分** |
| Web UI / Electron Desktop | 无 | 超范围 |
| npm bootstrap 分发 | 无（cargo 分发） | 分发模型不同 |

---

## 5. 上策（高价值，契合 memrec 治理与安全定位）

### 5.1 授权边界模型（三级独立授权）

**问题**：memrec 存储的是本地敏感记忆（代码、决策、偏好），但**没有授权模型**：任何能访问 socket 的进程都能读写全部记忆。

**机制**：按 MemHub 的三级边界拆分为独立授权——**来源读取**（从哪个 AI 应用读历史）、**模型处理**（是否允许送去 LLM 抽取）、**宿主读取**（哪个宿主可读哪些记忆）。三者互不隐含。

**落点**：`mr-protocol` 增加来源/宿主标识与授权位；`memrecd` 在检索时按授权过滤；配置持久化到 `config.toml`。

**收益**：这是 memrec 面向"多应用共享记忆"最欠缺、也最该有的一块；契合 AGENTS 中对安全性与可用性的要求。

### 5.2 显式单写入者契约 + 治理层

**问题**：memrec 事实上单写入，但无显式约束与文档；从 `Evidence`（archive）到 `Memory` 之后缺少 `Governance` 环节。

**机制**：
- 显式化"仅 `memrecd` 可写业务库"契约（禁止任何客户端直连数据库）。
- 在现有 `dream` 之上补一层治理：对合并、去重、跨项目提炼的结果做一致性/来源校验后再派生（`Matters/Wiki` 类比为可读的知识档）。

**落点**：文档与接口约束先行；治理检查可并入 `dream/phases/`。

---

## 6. 中策（工程纪律增强）

| 项 | 说明 | 落点 |
|---|---|---|
| 守卫式 `delete-data` | 独立命令、二次确认精确路径、仅在 daemon 停止后执行；卸载默认保留数据 | `mr-install` + CLI 子命令 |
| `doctor` 诊断聚合 | 汇总服务状态、模型、索引、连通性为单一诊断入口 | `memrec/src/commands/` 新增 |
| 平台支持合同 | 明确"仅在 Release 与 manifest 中且通过校验的制品才算支持" | CI + 文档 |
| 隐私边界文档 | 明确只存显式写入内容，不采屏幕/键鼠/剪贴板 | 新增 `PRIVACY.md` |
| 数据生命周期文档 | 安装/卸载/删除各阶段数据去向 | `docs/` |

---

## 7. 下策 / 超出定位

| 项 | 判定 | 理由 |
|---|---|---|
| Web 管理界面（React） | 不采纳 | memrec 为 CLI/daemon，界面非核心 |
| Electron 桌面壳 | 不采纳 | 同上 |
| 宿主接入层家族化（Host Bridge 适配器族） | 不采纳 | memrec 是唯一客户端入口，各宿主一律经由 memrec，无需再按 MCP/Skill/Hook 形态抽象适配器族（用户裁决 2026-10-17） |
| npm bootstrap 分发 | 不照搬 | memrec 用 cargo 分发；仅借鉴"显式 install、postinstall 无副作用"原则 |
| 多应用聚合 Hub 本身 | 谨慎 | memrec 的"一 daemon 多项目多客户端"已具雏形，先做强再谈聚合 |

---

## 8. 综合结论

- **记忆引擎侧**（记忆形态、任务域、版本链）看 Hy Memory，见另一文档。
- **产品与治理侧**（授权边界、单写入者、宿主接入家族化、数据生命周期纪律）看 MemHub。
- memrec **不应变成 MemHub**（不做 Hub/Desktop/Web），而应吸收其**纪律**：把"授权边界"与"显式契约"补上，这是当前最实质的短板。

优先级建议：先做 5.1（授权边界），它决定 memrec 能否安全地走向多应用共享；5.2 与中策项作为配套。

---

## 9. 风险与兼容性

- 授权模型涉及既有 `mr-protocol` 结构扩展，需保持向后兼容（新增字段带默认值）。
- 单写入者契约的强制化需评估现有 `memrec` CLI 是否存在任何绕过 daemon 的直连路径。
- `delete-data` 守卫需与 `archive/` 的"永不删除"语义协调：删除应覆盖存档，须明确声明。

---

## 10. 参考资料

- MemHub / One Memory README（发行与平台合同、核心原则、仓库结构、CLI 与 Runtime、隐私与安全）
- MemHub `docs/ARCHITECTURE.md`、`docs/contracts/runtime-unification.md`（外部仓库）
- memrec：`mr-install/`、`scripts/`、`dsh-plugin/`、`pi-extension/`、`SECURITY_*.md`
