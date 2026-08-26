# 更新日志

本文件记录项目的所有重要变更。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 新增

- **分页**：`ListParams.skip` / `memrec list --skip <n>` — 可翻页查看 limit 之外更早的记忆；`total` 现在返回符合条件的完整数量（准确判断是否还有下一页）
- **原始记录检索与恢复**：从 RocksDB 检索并恢复软删除记忆
  - `memrec list --deleted`：列出软删除的原始记录（RocksDB `cf_deleted` 列族）
  - `memrec list --tag <t> --type <t>`：标签/类型过滤（协议 `ListParams.tags`/`memory_type` 此前未生效，现已实现）
  - `memrec restore <id>`：恢复软删除记忆并重新加入混合检索索引

### 修复

- **Dream 垃圾记忆根治**（v0.7.0 遗留问题：`personal-summary`/`cross-project` 重复摘要污染记忆库，100 条中 94% 为机器生成）：
  - Phase 1/2 摘要改 upsert：每标签仅保留最新一条，内容变化才更新，多余旧条目自动回滚删除
  - DreamGate 时间门槛改用持久化 `dream.state`，与并发锁分离（此前 release 锁后时间信息丢失，24h 门槛失效导致高频执行）
  - Dream 统计摘要 `source=System`、importance 降至 0.3（此前默认 `User`/0.8，污染检索权重）
  - `process_inner` 先筛选旧记忆，数量不足（< `min_memories`）时跳过全部阶段（此前 phase 1/2 在 LLM 整合判断之前执行，整合被跳过时垃圾已写入）
  - Cleanup 增加摘要滚动清理兜底（每标签保留最新一条）
  - 存量清理：删除 1330 条重复摘要（active 1407 → 77）
- 修复 0.8.0-dev 版本 bump 时内部依赖版本未同步（`mr-common` 依赖声明 0.7.0-dev → 0.8.0-dev）

---

## [0.7.0] - 2026-08-21

### 新增

- **L2 场景聚合层**：基于主题聚合 L1 记忆卡片
  - `memrec scene` 子命令：`create` / `get` / `list` / `delete` / `add-memory` / `remove-memory` / `update-heat`
  - `Scene` 实体（`theme`、`memory_ids`、`heat`、`tags`），热度排序，记忆金字塔 L0-L3
  - `SceneStore` 基于 RocksDB，与 `MemoryStore` 共享实例
- **Dream 记忆整合**：后台记忆压缩与整合（类比睡眠记忆巩固）
  - 4 个阶段处理器：`CrossProjectExtract`（交叉项目提取）/ `PersonalSummary`（个人提炼）/ `Cleanup`（清理）/ `VectorRegen`（向量重生成，默认关闭）
  - `memrec dream --force` 手动触发；`DreamGate`（最少 20 条记忆、最大 168h 年龄、间隔 24h）+ `DreamLock`
  - LLM 摘要整合（需配置 `[llm]`），结果标记 `dream-integrated`
  - `SystemIdleMonitor` 检测系统空闲（load average < 0.5）时自动触发
- **LLM 接入（OpenAI 兼容）**：`[llm]` 配置节
  - `LlmClient` trait，含 `OpenAiCompatibleClient`（reqwest）与 `MockLlmClient`
  - provider 支持 `deepseek` / `openai` / `custom`；`think_level` 映射（DeepSeek `thinking` effort、OpenAI `reasoning_effort`）
- **写入去重**：每次 `add` 基于规则的决策
  - `score >= duplicate_threshold`（0.92）→ 跳过（NOOP），`>= update_threshold`（0.85）→ 更新，否则新增
  - CLI `--no-dedup` 标志与协议 `dedup: false` 选项；LLM 版决策器预留演进
- **SystemIdleMonitor**：基于 load average 的空闲检测（5 分钟间隔、阈值 0.5），空闲自动触发 Dream
- **QueueDispatcher**：双队列调度器（普通队列 1000 / Dream 队列 10）
- **日志滚存**：`[log]` 配置 — `memrecd.log` / `dream.log`，保留 7 天，可选压缩
- **dsh-plugin**：DeepSeek Harness 原生 MemRec 插件
  - 7 个原生工具：`mr_add` / `mr_search` / `mr_get` / `mr_list` / `mr_delete` / `mr_stats` / `mr_dream`
  - 上下文注入（`agent/pre-step` 钩子）+ 会话自动记录（LLM 提取，fire-and-forget）
  - 通过传递 `working_dir` 修复 MCP 模式丢失的项目隔离
- **pi 扩展**：`pi-extensions/memrec.ts`
  - 6 个 LLM 工具：`memrec_search` / `memrec_add` / `memrec_get` / `memrec_list` / `memrec_delete` / `memrec_stats`
  - agent 启动时自动记忆注入；`/memrec` 与 `/memrec-auto` 命令
- **线程命名**：tokio worker 线程命名
- 新增配置类型：`LlmConfig`、`DedupConfig`、`DreamConfig`、`IdleMonitorConfig`、`QueueConfig`、`LogConfig`

### 变更

- RocksDB 实例在 `MemoryStore` 与 `SceneStore` 间共享（单实例打开）
- `mr-install` 默认模型改为 BGE-M3，与 memrecd 保持一致
- `add` 参数封装为 `AddOptions` 结构体
- 测试改用 `MockEmbedder`；模型依赖测试标记为 ignored
- 日志写入路径解析与颜色渲染修复

### 修复

- 删除的记忆不再出现在混合检索结果中（删除时同步移除搜索索引）
- `min_score` 现在应用于融合后的混合分数
- 插件上下文注入过滤空 previews
- clippy 警告

### 文档

- `docs/design/L2-scene-aggregation-design.md`
- `docs/design/dsh-plugin-design.md`
- `docs/superpowers/specs/2026-07-30-idle-dream-design.md`
- `dsh-plugin/README.md`

---

## [0.5.0] - 2026-07-16

### 新增

- **混合检索**：KNN + BM25 全文检索，可配置 `--hybrid-alpha`（默认 0.5）
- **MMR 重排**：最大边缘相关度，结果多样性，`--mmr-enabled`（默认启用），`--mmr-lambda`（默认 0.7）
- **时间衰减评分**：近期记忆权重更高，可配置衰减率
- **常青豁免**：knowledge/decision 类型豁免时间衰减
- **来源权重**：用户记忆权重高于系统/推断
- **中文搜索**：N-gram 分词器（2-4 字）支持中文全文检索
- `MemorySource` 枚举：`User`、`System`、`Inferred`、`External`
- `MemoryScope` 枚举：`Project`、`Global`、`Workspace`
- `memrec add` 新增 `--source` 和 `--scope` 参数
- `SearchConfig`、`SourceWeights` 配置类型
- `HybridStorage` trait 和 `HybridStore` 实现
- `FtsStorage` trait 和 `TantivyStore`（Tantivy 0.22）BM25 搜索

### 变更

- 搜索流程：KNN+BM25 并行 → 合并归一化 → 时间衰减+来源权重 → MMR 重排
- 搜索结果新增 `created_at` 字段
- `memrecd` 二进制大小：42MB → 47MB（Tantivy 依赖）
- QueryParser 需要 `IndexRecordOption::WithFreqsAndPositions` 支持 n-gram 分词

### 修复

- `source`/`scope` 参数未从 AddParams 传递到 Memory 实体
- Tantivy QueryParser 错误："field does not have positions indexed"

### 不兼容变更

- Tantivy schema 变更：升级前删除 `~/.memrec/fts/`，索引将自动重建

---

## [0.4.0] - 2026-07-16

### 新增

- 多模型嵌入支持：安装时选择 MiniLM-L6-v2（384维，英文）或 BGE-M3（1024维，多语言/中文）
- `mr-install` 新增 `--model` 参数：`--model bge-m3` 或 `--model minilm-l6-v2`（默认）
- common 中新增 `ModelType`、`ModelConfig`、`ModelFile`、`ModelFileType`、`PoolingStrategy` 类型
- `FastEmbedGenerator` 统一使用 `UserDefinedEmbedding` + 外部初始化器 + 可配置池化（BGE-M3 用 CLS，MiniLM 用 Mean）
- 模型驱动下载：`mr-install` 读取 `ModelConfig` 确定下载文件，支持 SHA-256 校验
- `mr-install` 新增 `--skip-hash-verify` 和 `--allow-any-repo` 参数
- `ModelType` 新增 `default_min_score()`：MiniLM 为 0.75，BGE-M3 为 0.5（BGE-M3 余弦分数天然较低）
- 新 config.toml 格式：`[model]` 节包含 `model_type`、`source`、`dimension`、`[[model.files]]` 数组
- `DaemonConfig` 中嵌套 `DaemonServerConfig`，支持 `expand_tilde` 路径展开
- `ModelFileType` 支持 `sentencepiece.bpe.model`（BGE-M3 分词器）
- 完整用户手册：`MANUAL.md`（英文）和 `MANUAL_cn.md`（中文）

### 变更

- 默认 min_score：0.75 → 0.5（BGE-M3 产生较低余弦相似度；MiniLM 仍通过 `ModelType::default_min_score()` 使用 0.75）
- 配置格式：扁平 `name = "Qdrant/all-MiniLM-L6-v2-onnx"` → 结构化 `[model]` 节含 `model_type` 和 `[[model.files]]`
- `mr-install` 根据所选模型类型生成配置
- 向量维度：384（MiniLM）或 1024（BGE-M3），由模型选择决定
- 运行时内存：~118MB（MiniLM）/ ~1.5GB（BGE-M3）
- 模型下载大小：~90MB（MiniLM）/ ~2.3GB（BGE-M3）
- 文档整合：`docs/user-guide.md` 和 `docs/installation.md` 合并为 `MANUAL.md` / `MANUAL_cn.md`
- README 文档链接更新指向 MANUAL 文件

### 修复

- BGE-M3 搜索返回 0 结果：调整默认 min_score 从 0.75 到 0.5
- 外部 ONNX 数据加载：`with_external_initializer()` 支持 BGE-M3 的 `model.onnx_data` 和 `Constant_7_attr__value`
- BGE-M3 池化：使用 CLS 池化替代 Mean 以获得正确嵌入

### 移除

- 旧扁平配置格式（config.toml 中的 `name = "Qdrant/all-MiniLM-L6-v2-onnx"`）
- 硬编码模型路径假设
- `docs/user-guide.md`（已被 `MANUAL_cn.md` 取代）
- `docs/installation.md`（已被 `MANUAL.md` / `MANUAL_cn.md` 取代）

---

## [0.3.0] - 2026-06-23

### 新增

- `mr-install` crate：一站式安装器，支持 `cargo install`、模型下载、服务注册和验证
- `ServiceManager` trait，含 Linux（systemd）和 macOS（launchd）实现
- 平台特定默认二进制目录（Linux `~/.local/bin/`，macOS `~/bin/`）
- 默认从 crates.io 安装，`--repo-url` 指定 Git 源
- `mr-install` 安全加固：SHA-256 模型校验、仓库 URL 白名单、`--allow-any-repo` 覆盖
- `memrecd` 新增 `--version` 参数
- 所有子项目（common、memrec、memrecd、mr-install）的完整 README 文档
- 安全分析文档：`SECURITY_ANALYSIS.md`、`SECURITY_HIGHLIGHTS.md`、`SECURITY_SUMMARY.md`

### 变更

- 安装方式：`install.sh` → `mr-install`（Rust 实现，跨平台）
- `docs/installation.md` 更新为 `mr-install` 工作流

### 修复

- `memrec get --merge` 对非分块记忆直接返回内容而非报错
- 服务器处理器改进

### 移除

- Windows 支持（因 rocksdb bindgen/clang 依赖移除）
- `install.sh`（被 `mr-install` 取代）
- `docs/systemd.md`（合并到 `docs/installation.md`）

---

## [0.2.0] - 2026-05-14

### 新增

- MCP 服务器：`memrec --mcp` stdio 模式，AI 客户端通过 MCP 协议直接调用
- 6 个 MCP 工具：mr_add、mr_search、mr_get、mr_list、mr_delete、mr_stats
- 2 个 MCP 资源：memrec://stats、memrec://project
- 项目隔离：客户端传递 working_dir（优先 git root），服务器从 .mr_pid 检测 project_id
- 跨项目搜索：`memrec search "query" --all` 搜索所有项目
- 搜索命令新增 `--all` 标志
- `SearchMemoryParams` 新增 `cross_project` 字段
- `AddParams`、`SearchMemoryParams`、`GetProjectInfoParams` 新增 `working_dir` 字段
- 参数化 min_score 默认值：`MEMREC_MIN_SCORE` 环境变量（默认 0.75）
- common 库导出 `default_min_score` 和 `default_include_global`
- 安装脚本：`install.sh`（构建、安装、模型下载、systemd、测试）
- 安装指南：`docs/installation.md`
- 用户指南：`docs/user-guide.md`
- Apache 2.0 许可证

### 变更

- min_score 默认值：0.0 → 0.75（过滤低相关性噪声）
- 向量存储目录：`~/.memrec/data/vectors/` → `~/.memrec/vectors/`（与元数据分离）
- knowledge 类型支持基于 tag 的子分类：fact、best-practice、algorithm、tool
- SearchArgs 字段设为 public 供 main.rs 访问
- README 更新：项目隔离、语义搜索、跨项目搜索、MCP、数据位置

### 修复

- 项目隔离：客户端现在传递 working_dir，服务器使用 .mr_pid 检测（之前所有记忆写入同一 project_id）
- 缓冲区溢出：动态响应缓冲区（1MB 限制），服务器发送后关闭
- RocksDB 锁：测试在重新打开前使用 drop 作用域
- Clippy：0 警告

### 移除

- qdrant-client 依赖（未使用的占位符）
- qdrant.rs 和 persistent_vector_store.rs（未使用）
- memrec-skill-phase6.md（过时草稿）

---

## [0.1.0] - 2026-04-23

### 新增

- 基于 RocksDB 存储后端的记忆持久化系统
- Unix Socket 守护进程（memrecd），JSON-RPC 2.0 协议
- CLI 工具（memrec）用于记忆管理
- 5 种记忆类型：decision、knowledge、context、preference、conversation
- 基于 tag 的分类与重要性加权
- 自动长内容拆分（>7.5KB）
- FastEmbed 真实语义嵌入（all-MiniLM-L6-v2，384 维）
- 本地 ONNX 模型加载（~90MB）
- 通过 MEMREC_MODEL_DIR 环境变量配置模型路径
- 基于 RocksDB 的向量持久化
- 启动时自动重建缺失嵌入
- 30 秒同步间隔 + 优雅关闭保存
- systemd 用户服务，含安装/卸载脚本
- AI CLI 工具 Skill（opencode、claude code）
- 中英文 README
- 52 个单元测试
