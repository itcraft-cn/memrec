# 更新日志

本文件记录项目的所有重要变更。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

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
