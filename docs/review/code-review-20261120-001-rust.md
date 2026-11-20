# memrec 代码审查报告（Rust）

- **日期**：2026-11-20
- **范围**：全量审查 Rust workspace（6 个 crate / 164 个 `.rs` / 37,250 行）＋ DSH 插件（TypeScript 1,317 行）
- **审查依据**：`${AI_SPEC_ROOT}/lang-spec/review.rust.md`（v1.3.0）、`skills/code-review/SKILL.md`
- **基线提交**：`ad43f87`（style: apply cargo fmt + clippy 清零）
- **方法**：先跑静态分析，再按模块分 6 组并行人工审查，最后对全部 P0 结论逐条回读源码复核（复核状态见各条「核验」）

## 一、静态分析结果

| 工具 | 命令 | 结果 |
|---|---|---|
| 格式 | `cargo fmt --all -- --check` | **通过**（本次审查前刚清零） |
| 缺陷/惯用法 | `cargo clippy --all-targets -- -D warnings` | **通过**（0 告警） |
| 行为 | `cargo test --release` | **通过**（16/16 套件） |
| 依赖漏洞 | `cargo audit` | **未执行成功**：沙箱只读导致 advisory DB 锁失败（环境限制，非项目问题） |
| 许可证/依赖策略 | `cargo deny check` | **未安装** |
| UB/unsafe | `cargo miri test` | **不适用**：依赖 RocksDB C++ FFI，miri 无法运行 |

补充事实：全仓仅 1 处 `unsafe`（`memrec/src/main.rs:250` 恢复 SIGPIPE 默认处置），带 `// SAFETY:` 注释，合规；无 `transmute`/`get_unchecked`/`Box::leak`；`mem::forget` 仅出现在 `#[cfg(test)]` 构造器（`memrecd/src/server/handler/mod.rs:219`、`unix_socket.rs:171`），属保留临时目录的有意行为。

## 二、P0（本次必须修复，9 条）

> 全部 9 条均已由我回读源码逐条复核确认，非仅转述。

### P0-1 守护进程单次 `read` 只读 8192 字节，超长请求被静默截断

- **位置**：`memrecd/src/server/unix_socket.rs:89-96`
- **证据**：`let mut buffer = vec![0u8; 8192];` 后仅 `stream.read(&mut buffer)` 一次，没有累计读取或长度分帧。
- **影响**：SOCK_STREAM 不保证一次读完。任何超过 8192 字节的请求后半段被丢弃 → `parse_request` 失败 → 客户端只看到解析错误。CLI 用 `MAX_CHUNK_SIZE = 7500`（`memrec/src/commands/memory/mod.rs:16`）对正文分块规避，但 **MCP `mr_add` 与 DSH 插件不分块**，正文超过约 7.8KB（含 JSON 转义膨胀）必然写入失败；`list -a -l 400` 等大读请求同样断连。这是当前写入路径最实际的容量天花板。
- **核验**：已复核源码；客户端 `client/connection.rs:57` 确为一次性 `write_all` 无分片。
- **修复**：`handle_connection` 改为循环读直到 `serde_json::from_slice::<JsonRpcRequest>` 成功或达到显式上限（建议 `MAX_REQUEST_BYTES = 1MB`，与客户端响应上限对齐），超限返回 `-32600`；读取阶段包 `tokio::time::timeout`。
- **验证**：新增服务端集成测试，分别发送 8KB/64KB/1MB 的 add 请求断言成功，200KB 超限请求断言返回 `-32600`；再用 MCP `mr_add` 写入 20KB 正文断言可 `mr_get` 读回。

### P0-2 连接无读超时、无并发上限，慢连接可耗尽 fd

- **位置**：`memrecd/src/server/unix_socket.rs:66-75`（accept/spawn）、`:89`（无 timeout 的 read）
- **影响**：任一本地进程建立连接后不发数据即永久占用 1 个 fd + 1 个任务；重复连接可耗尽 fd，之后 accept 全部失败，进程「活着但不响应」，只能重启恢复。
- **核验**：已复核，`tokio::spawn` 无信号量，`read` 无 timeout。
- **修复**：`Semaphore` 限制并发连接（建议 128），超额直接关闭；`handle_connection` 读写各加 timeout（读 30s、写 60s）；spawn 内统一 `warn!` 记录连接级错误。
- **验证**：并发建立 2000 个只连不发的客户端，`/proc/<pid>/fd` 数量应在 30s 后回落到上限内（修复前单调增长）。

### P0-3 `.mr_pid` 采用「exists → 生成 → write」非原子创建，并发首写产生两个 project_id

- **位置**：`mr-ability/src/project/detect.rs:132-140`
- **核验**：已复核；调用点 `memrecd/src/server/handler/memory_write.rs:89` 每次写入都可能触达，连接层按连接 `tokio::spawn`（`unix_socket.rs:72`）确实并发。
- **影响**：两个并发首次写入各生成一个 UUID，后写者覆盖文件；先写者以其未落盘的 UUID 入库，此后按项目检索永远查不到，形成不可见的项目分叉。`fs::write` 崩溃还会留下半截文件被判为无效。
- **修复**：`OpenOptions::new().create_new(true)` 独占创建 + `sync_all`；`AlreadyExists` 时回到读取分支重试（≤3 次），保证返回值必为文件中的 UUID。
- **验证**：`tokio::join!` 并发 200 次 `detect_project_id`（同一临时 git 目录），断言所有返回值相等且与文件内容一致。

### P0-4 `ModelConfig::default()` 自相矛盾，首次部署确定性故障

- **位置**：`mr-common/src/types/model.rs:296-306`
- **证据**：`model_type: ModelType::default()`（默认变体为 `BGEM3`，见 `:32-37`）与 `dimension: ModelType::default().dimension()`（1024）配 `files: ModelFile::for_minilm()`（MiniLM 的 5 个文件）。
- **核验**：已复核，`ModelType` 的 `#[default]` 确为 `BGEM3`，文件中同时存在 `for_minilm()` 与 `for_bge_m3()`。
- **影响**：`config.toml` 缺失时（全新部署）`DaemonConfig::default()` 被使用并被 `load()` 写盘。按 `model_type` 推出的缓存目录是 `BAAI--bge-m3`，却校验 MiniLM 文件名 → 报「模型文件缺失」；即便文件凑齐，向量维度 1024 与 MiniLM ONNX 输出的 384 不匹配，`rocksdb_vector.rs:145` 的维度校验会让所有向量写入失败，且错误配置已持久化，重启不自愈。
- **修复**：`Default` 改为委派 `ModelConfig::new(ModelType::default())`，消除两处独立取默认；`is_ready()` 增加 `dimension == model_type.dimension()` 与 `files` 匹配校验。
- **验证**：单测断言 `ModelConfig::default().local_dir_name() == "BAAI--bge-m3"` 且 files 的 `remote_path` 属于 bge-m3；临时 HOME 下删除 config.toml 启动 memrecd 后 `add`/`search` 成功。

### P0-5 `--ttl` 数值无上界，`chrono::Duration::seconds()` 越界 panic 可直接打崩 CLI 与 MCP 服务器

- **位置**：`memrec/src/commands/memory/crud.rs:52`（`n * unit_secs` 未 `checked_mul`，`Duration::seconds` 越界即 panic；`Utc::now() + dur` 溢出亦 panic）
- **核验**：已复核 `parse_ttl` 实现；MCP 侧 `mcp/tools.rs` 同样调用该函数，且运行在长驻 stdio 进程里。
- **影响**：`memrec add x --mtype knowledge --ttl 9223372036854776s` 直接 panic（非零退出可以接受）；但经 MCP 传入同一参数会打死 MCP server，模型侧表现为连接中断、后续所有工具调用丢失。release 下乘法回绕为负则过期时间落在过去，写入即过期，属静默数据错误。
- **修复**：`checked_mul` + 显式上界（如 ≤10 年）+ `chrono::Duration::try_seconds` + `Utc::now().checked_add_signed`，全链路返回 `Result`。
- **验证**：单测 `parse_ttl("9223372036854776s")` 与 `parse_ttl("2000000000000000000d")` 均 `is_err()` 且不 panic。

### P0-6 MCP `mr_add` 不检查守护进程 error，写入失败一律回「成功」

- **位置**：`memrec/src/mcp/tools.rs:259-269`
- **证据**：`call_daemon(...).await?` 只覆盖传输错误，随后直接 `resp.get("result")...unwrap_or("unknown")` 并返回 `Added memory: unknown`。
- **核验**：已复核，函数体内无 `resp.get("error")` 判定（同文件 `tool_dream` 有）。
- **影响**：去重跳过、项目归属失败、测试模式 `-32010`、存储故障等一律被当成成功。这是模型唯一写入口，静默丢失会导致「记忆没写进去但以为写了」，后续跨会话依赖全部落空。同类问题见 P1-2（读类工具）。
- **修复**：抽 `fn ensure_ok(resp: &Value) -> Result<(), String>`，命中 `error` 即 `Err`；`content` 空值、`memory_type` 非法改为报错而非静默回退；仅当 `result.memory.id` 存在才拼成功文案。
- **验证**：构造返回 `{"error":{"code":-32010}}` 的 daemon stub，断言 `isError: true`；测试模式下 `mr_add` 必须失败。

### P0-7 Dream 锁获取是 TOCTOU 且写入非原子，两入口可同时执行 Dream

- **位置**：`mr-ability/src/dream/lock.rs:86-99`（`exists()` → `read_lock()` → `File::create` 写入）；损坏锁文件使 `read_lock()?` 直接失败
- **核验**：已复核；两个入口 `daemon/mod.rs` 的 `dream_loop` 与 `handler/dream.rs:18` 的 RPC 均可触发。
- **影响**：两方都判定「无锁」后都执行，重复产出 facts/summary、重复写边、重复标记 processed。次生问题：`File::create` 中途崩溃留下截断 JSON，此后每次 `try_acquire` 返回 Err，Dream 被永久拒绝直到人工删文件。
- **修复**：改 `OpenOptions::new().create_new(true)`，用 `AlreadyExists` 分支代替 `exists()` 预检；写内容前先写临时文件再 `rename`；`read_lock` 解析失败按僵锁处理（warn 后清理重试），不得上抛。
- **验证**：16 个并发任务 `try_acquire`，断言成功者恰为 1；预置截断锁文件断言 `try_acquire` 返回 `Ok(true)`。

### P0-8 Dream 锁无 RAII 守卫，提前返回或 panic 后锁永久泄漏

- **位置**：`mr-ability/src/dream/processor/mod.rs:258`（acquire）→ `:268`（`self.storage.count().await.map_err(...)?` 提前返回）→ `:294`（`release()`）
- **核验**：已复核该 `?` 位于 `release()` 之前；`process_inner` panic 同路径。
- **影响**：锁文件残留，而 `is_process_alive` 检查的是当前仍存活的 daemon 进程 → 此后每次 `try_acquire` 恒 `Ok(false)`，Dream 永久停摆，只表现为 `GateFailed("Another Dream process is running")`，无其他告警。
- **修复**：`try_acquire` 返回 `LockGuard<'a>`（`Drop` 中 `let _ = release()`），以 `let _guard = ...` 绑定到函数作用域并删除末尾显式 release；锁内容加时间戳，超时视为僵锁。
- **验证**：注入让 `storage.count()` 返回 Err 的 store，断言函数返回后锁文件已消失。

### P0-9 Dream LLM 解析失败时回退「模板摘要 + 原文逐条 facts」并照常写库

- **位置**：`mr-ability/src/dream/fusion.rs:290-299`（解析失败仅 `warn!` 后返回 fallback）、`:352-366`（`fallback_extraction` 生成 `[Dream 分簇摘要] 本簇整合了 N 条记忆。` 并把簇内原文全部复制为 facts）
- **核验**：已复核两处代码。
- **影响**：每次解析失败都写入 1 条模板摘要 + N 条原文副本，标注 `System/Global` + `dream-integrated`，且 `is_dream_generated()` 使其不再进入后续 Dream 候选 → 永久沉积、污染检索、稀释真实记忆（与历史上「Dream 垃圾记忆」同源，此前修复未覆盖该回退路径）。
- **修复**：`extract_cluster` 区分 `Success/Fallback`；`Fallback` 时本轮跳过该簇（不写产出、不 `mark_processed`），在 `DreamRun` 记 `parse_failed_clusters`；`summary` 为空或空白一律不写库（见 P1-6）。
- **验证**：注入返回非 JSON 的 `MockLlmClient` 跑完整 `execute`，断言新增记忆数为 0 且源记忆无 `dream-processed` 标签。

### P0 判定说明（两条降级）

- `propagation/engine.rs:131`、`rule/engine.rs:72`、`edge_store.rs:352` 的 `partial_cmp().unwrap()`：agent 判为 P0，我**降级为 P1**。理由：`serde_json` 无法表示 NaN，持久化数据不可能含 NaN；当前无计算路径产出 NaN（已 grep 除法/ln 构造），故不可达。但应改用 `total_cmp` 零成本消除该类崩溃面（P1-9）。
- `queue/dispatcher.rs` 丢弃响应：模块在生产装配中无调用方（未接线），按「影响决定级别」定为 P1。

## 三、P1（下次迭代修复，按主题归并，29 条）

### A. 项目隔离与过滤契约

1. **图扩散命中绕过 project/memory_type 过滤** — `handler/search.rs:196-232` 仅判 `is_deleted`，邻居来自无项目条件的 `GraphTraversalParams`。`project_only=true` 仍可能返回其他项目记忆（我已复核）。修复：图分支复用与种子一致的 `SearchFilter` 判定。
2. **MCP 读类工具把 daemon 错误当成功** — `mcp/tools.rs:191-206` 分发处统一 `isError:false`，仅 dream 系检查 error；`mr_get`/`mr_list`/`mr_delete` 失败时模型无法感知。
3. **`include_global` 协议字段被服务端忽略** — `mr-protocol/.../memory.rs:98-100` 定义但无读取，handler 自行推导（`search.rs:48/383`）。third-party 传 `include_global` 无效且无法表达「仅项目+全局」组合。
4. **CLI `-p` 时 `.mr_pid` 解析失败退化为「全部非全局记忆」** — `main.rs:231-235` 以 `read_project_uuid` 的 `None` 下发，跨项目泄漏；`detect_project_id(...).ok()`（`memory_write.rs:89`）同样把坏文件降级为全局写入且无日志。
5. **写入去重跨项目** — `memory_write.rs:320-332` 用 `project_id: None` 的向量检索做去重，A 项目已有同文本时 B 项目 add 会 `skipped` 且不写入；`embed().ok()?`/`search().await.ok()?` 还把故障折叠成「无候选」。

### B. 并发与资源

6. **Dream 摘要空/空白仍写库** — `processor/pipeline.rs:396-411`，`{"facts":[],"summary":""}` 是 LLM 常见合法输出，产生空内容 + `importance=0.8` 记录，检索不可见但占库。
7. **簇内部分写入失败导致整簇被判已整合** — `pipeline.rs:336-346` 在首条产出前写入覆盖整簇的 `source_ids`；中途失败后下一轮把整簇排除，缺失 facts 永久丢失。
8. **`batch_size = 0` 让 `chunks(0)` panic** — `phases/cleanup.rs:64`、`vector_regen.rs:57`，配置可写成 0，panic 发生在持锁路径上（叠加 P0-8 变成永久锁死）。
9. **Dream 每轮全量装载候选** — `pipeline.rs:205-209` `list_created_between(0, cutoff, usize::MAX)` 后只取 100 条；`metrics.rs:169-173` 用 `list_by_tag(TAG_INTEGRATED, usize::MAX)` 全量拉历史整合记忆。
10. **`apply_time_decay` 无「未来时间戳」与零半衰期防护** — `search/scorer.rs:86-89`：`created_at > now` 时分数大于入参（衰减变增益），`decay_half_life_hours = 0` 产生 NaN 并静默传播；同文件的 `apply_ranking_modifier` 已有 `.max(0)`，两处口径不一致。

### C. 异步阻塞与锁

11. **`rocksdb_vector.rs:89-114` 每次脏刷新全量重写 + 锁内 I/O** — 单布尔 `dirty` 使任意一条 add/remove 触发全量 JSON 序列化与 `db.write`（含 fsync），全程持 cache 锁；1024 维单条约 20KB，1 万条约 200MB 写放大，且搜索被阻塞。
12. **`FastEmbedGenerator::embed` 在 `std::sync::Mutex` 内同步推理，由 async 直接调用** — `embedding/fastembed.rs:171-197`；Dream 的 `metrics.rs:98`、`vector_regen.rs:61` 逐条调用，`embed_batch` 仅一处使用，序列化全部并发检索。
13. **Tantivy 每条记忆一次 `commit`（含 fsync）在 tokio worker 上执行** — `tantivy_store.rs:438-456`；`indexing.rs:127-141` 的 reindex 先 remove 再 add 等于两次 commit。全目录各 store 均为 async fn 内同步 RocksDB I/O，属系统性设计。

### D. 静默降级与错误丢失

14. **索引写入失败仍返回成功** — `memory_write.rs:278-285`（`.ok()`）、`memory_read.rs:356-365`；`rebuild_missing_embeddings` 以「计数相等」早退，可能永久跳过修复。
15. **检索元数据回表失败伪造成默认值** — `search.rs:87-97` 返回 `Conversation` 类型与 `1970-01-01` 时间；`get_many` 错误被 `unwrap_or_else` 丢弃。
16. **向量库 `open()` 静默丢弃坏记录** — `rocksdb_vector.rs:67-75` 三层 `if let Ok` + `.flatten()`，损坏向量使该记忆永久不可检索且无日志。
17. **维度不匹配时余弦用 `zip` 静默截断** — `rocksdb_vector.rs:117-127` 配合 `:144-151`：换模型后旧向量参与排序，结果错误且无异常。
18. **Tantivy 重建失败仍写版本标记** — `tantivy_store.rs:187-208`：`remove_dir_all` 失败被吞后仍写 marker，下次启动不再重建，不可自愈。
19. **索引中非 UUID 文档退化为 `Uuid::nil()` 并作为正常命中返回** — `tantivy_store.rs:489-505`（debug 级日志，生产不可见），nil 命中互相覆盖。
20. **RocksDB 迭代器从不检查 `status()`** — `rocksdb.rs:362-371` 等 5 处：I/O 错误被当成正常结束，`count()`/`list()` 返回不完整结果；Dream 门控的记忆总数判定可能因此误判。
21. **`llm/client.rs:166-169` 响应体无大小上限、全链路无重试** — 单个超大响应可顶高 RSS；429/5xx 使整轮 Dream 失败。
22. **`edge/generator.rs:299-306` LLM 调用失败 `Err(_) => Ok(Vec::new())`**，零日志，与同文件 `parse_relations` 已修正的方向矛盾。
23. **`rule/matcher.rs:38-48` 正则编译失败回退 `.*`（匹配一切）** — 非法规则变成全局命中，若 action 为 Filter/Suppress 则「检索突然什么都搜不到」；模式缓存只增不删。

### E. 协议与 CLI 行为

24. **CLI 收到 JSON-RPC 错误仍 `Ok(())`，退出码恒 0** — `crud.rs:185-187`、`search.rs:105-109`；`search --human` 连错误都不打印。脚本与 Agent 无法按退出码判失败。
25. **CLI 客户端无连接/读取超时** — `client/connection.rs:50-89`；`mr-install/download.rs:67` 的 `reqwest::get` 亦无 timeout。
26. **MCP stdin 单行无长度上限，非 UTF-8 或写失败直接终止服务器** — `mcp/mod.rs:91-101`、`:141-145`。
27. **分块 add 每块不检查 error，失败仍宣告「All N parts added」** — `crud.rs:116-131`。
28. **非法 UUID/枚举被静默降级为「不筛选」** — `main.rs:428/440`、`search.rs:67-74`、`mcp/mod.rs:49`；`--top-k 0`、`--min-score 5`、`--min-score nan` 原样透传。
29. **协议参数无上界** — `limit`/`top_k`/`skip`/`content`/`tags.len()` 均无约束：`top_k = usize::MAX` 时 `p.top_k.max(1) * 3`（`search.rs:136`）在 release 下回绕，`limit = usize::MAX` 等于全量导出（`memory_read.rs:184-205`，且默认分页也走全量加载）。

## 四、P2（选择性修复，摘要）

1. `handler/memory_read.rs:21-27`：`Get` 是读请求却内部 `storage.update()`，写放大且使「测试期禁写」在 Get 上失效（`test_mode.rs` 把 Get 归为读）。
2. `handler/dream.rs:13-16`：`params` 类型不匹配静默取默认并**真正执行 Dream**（写库/软删/建边），应返回 `-32602`。
3. `handler/memory_read.rs:288-310`：`Delete` 忽略 `force`；目标不存在也返回「已删除」成功。
4. `unix_socket.rs:41-43`：绑定前无条件 `remove_file` 旧 socket，无单实例保护，误启第二实例会抢走全部流量并删掉新实例 socket。
5. `unix_socket.rs:99-107`：解析失败用 `format!("{{\"error\": ...}}")` 拼 JSON，缺 `jsonrpc`/`id`，错误码退化为字符串，客户端无法反序列化。
6. `mr-protocol` 响应 `DeleteParams.force`/`ProjectDeleteParams.force` 无消费方；`MemoryResult.action` 是自由字符串且缺省伪造为 `"added"`。
7. `mr-protocol/request/mod.rs:126-128` + `unix_socket.rs:106`：未知 `type` 的内标签反序列化失败路径不返回合法 JSON-RPC 响应。
8. `mr-common` 死配置：`retrieval.injection_max_tokens`、`memory.compression_importance/max_storage_gb/high_watermark/low_watermark`、`ModelType::default_min_score`、`SearchConfig` 全部无消费方（运维改配置不生效且无告警）。
9. `mr-common/types/llm.rs:90-127`：`LlmConfig` 派生 `Debug` 且含明文 `api_key`；`temperature`/`timeout_secs`/`max_tokens` 无范围校验（TOML 允许 `nan`）。
10. `memrecd/src/main.rs:129`：`eprintln!` 输出日志清理失败，违反项目「统一用日志」约定；`Daemon::shutdown` 不清理 test socket。
11. `mr-install/src/download.rs:115-118`：`all_files_exist` 提前返回绕过 SHA-256 校验，中断残留的半成品被当「已就绪」；`install.rs:37` 的安装清单含不存在的 `memrec-common`。
12. `mr-install/src/verify.rs:25-54`：验证解析的是 JSON 输出却匹配 `--human` 文案，`test_id` 恒 `None` → 每次安装都在真实库留一条 `test` 垃圾记忆。
13. `mr-install/src/config.rs:58-61`：生成的 `config.toml` 未设 0600（含 `[llm] api_key`）且无条件覆盖既有配置。
14. `mr-install/src/launchd.rs:114-122`：`launchctl bootstrap` 用字面量 `~` 路径（不经 shell 不展开），macOS 上稳定失败；systemd 单元路径未加引号。
15. `search/mmr.rs:84-90`：`tokenize` 对中文退化为整句一个 token，Jaccard 恒 0 → **中文语料下 MMR 多样性完全失效**（项目主体语料是中文，可复用 `cn_tokenizer`）。
16. `dream/phases/personal_summary.rs:62-78`：摘要正文按 `HashMap` 迭代顺序拼接，每轮内容不同 → 每次都触发无谓覆盖写与索引重建。
17. `rule_store.rs:50-63`：保存规则无条件覆盖统计列族，命中计数被清零。
18. `scene_store.rs:101-123`：`save_scene` 不复写旧索引，改 theme/heat 后旧键残留（`get_scene_by_theme` 还能取到软删场景）。
19. `memory_store/crud.rs:142-161`：硬删除只清 3 个列族，`by_created`/`by_project`/`importance` 残留死键。
20. `memory_store/crud.rs:222-251`：`list_by_tag` 用 `seek_to_first` 全表扫描 + 循环内 `self.get` 的 N+1。
21. `expiry.rs:46-48`：每 300s 用 `list_created_between(0, now, usize::MAX)` 全量加载做 TTL 扫描（底层逐条 `get`），随时间线性劣化。
22. `tiered/budget.rs:135-152`：类型优先分配用 `Vec::remove` 反复搬移，O(n²)。
23. `tiered/engine.rs:93-97`：每条结果串行 N+1 回表（`neighbors` + ≤3 次 `get_summary`）。
24. `propagation/engine.rs:147-158` + `apply_pending` 无调用方：只扫前 1000 条记忆的 pending，超出者永久饥饿；生产装配了 `MockDeltaGenerator` 持续写无效 delta。
25. 未接线模块：`queue/`（10ms 忙轮询且丢弃响应）、`rule/engine.rs`（`Boost`/`Suppress` 实现相同、`Expand` 空分支、`edge_storage` 字段从不读取）、`lifecycle/`、`TieredEngine` 泛型包装与 `BudgetController::check_and_downgrade` 均无生产调用方。
26. 协议声明但无 handler 的 9 个 `RequestAction`：`Search`/`Update`/`Tag`/`ProjectCreate`/`ProjectSwitch`/`ProjectDelete`/`ProjectList`/`ConfigGet`/`ConfigSet`（已用 grep 确认 handler 命中数为 0），统一落 `-32601`；`is_write_action` 却对其做读写归类。
27. 错误码语义冲突：`-32000` 同时表示通用存储错误与「创建场景失败」，`-32005` 同时表示「无分块」与「分层检索错误」，11 个自造码散落在 5 个文件的手写字面量里。

### DSH 插件（TypeScript，自查）

- **P1**：`context.ts:210/212/213`、`lifecycle.ts:118-120` 共 6 个以 `agentId`/`sessionId` 为 key 的 `Map`/`Set` 永不清理。Web 进程长驻且子代理会大量创建 agent，属无界内存增长。修复：监听 `session/disposed` 清理，或改用有界 LRU。
- **P2**：`context.ts:267` 用 `console.warn`，而 `lifecycle.ts` 用 `ctx.logger`，同一插件两套日志通道且违反项目「不用 stdout/stderr」约定。
- **P2**：`tools.ts` 的参数面仍用 `project_only`/`global_only`/`cross_project` 三个布尔，与 MCP 已统一到的 `scope` 枚举不一致，两套客户端语义漂移（`include_global` 的推导差异正是 P1-3 的诱因）。
- **P2**：`context.ts:121-149` `fetchRuleSets` 串行 2-3 次 socket 往返（`list` → `get_project_info` → `list`），且 `.catch(() => [])` 完全静默（注入失败有 warn，规则拉取失败没有）。

## 五、模块级观察

- **文件长度**：无文件超 1000 行（最大 `hybrid_store.rs` 805 行，其中 387 行是内联测试；`mr-protocol/response.rs` 779 行含 16 个协议结构与聚合逻辑）。`hybrid_store.rs` 把测试外置即可回到 500 行内；`response.rs` 的 `DreamStats::from_runs` 属业务计算，建议拆 `response/stats.rs`。
- **重复逻辑（≥3 处）**：①「遍历 + `if let Ok` 反序列化 + 过滤已删」在 `memory_store/mod.rs`、`scene_store.rs`、`dream_run_store.rs`、`edge_store.rs` 四份；②「`seek_to_first` + 手写前缀扫描」四份，而 `propagation_store.rs:109`/`migration.rs:16` 已示范正确的 `seek(prefix)`——建议在 `rocksdb.rs` 提供 `scan_prefix()` 统一封装，可一并修掉 P1-20；③批处理骨架 `chunks(batch_size)` + sleep 在 `cleanup.rs` 与 `vector_regen.rs` 逐行重复；④余弦相似度两份（`fusion.rs` 带 clamp，`processor/helpers.rs` 不带），阈值边界行为不一致；⑤`scene.rs` 6 个 handler 同一模板。
- **配置真相分裂**：`mr_common::SearchConfig` 无消费方，`mr-ability/search/scorer.rs` 另有一套结构相同的 `SearchScorerConfig`，`evergreen_scopes` 类型还从 `Vec<String>` 漂移到 `Vec<MemoryScope>`。「配置改了不生效」是系统性问题（见 P2-8）。
- **Router 是上帝结构**：20 个字段、`impl` 分散在 5 个文件、字段靠 `pub(super)` 跨文件共享；新增方法要同时改 `route` 的 match、`test_mode::is_write_action` 和对应 handler，漏改第一处只会在运行期落到 `-32601`，编译期无提醒。
- **公开面**：`rocksdb.rs` 暴露 25 个 `pub fn cf_*` 列族句柄给 crate 外，外部可绕过索引维护直接 put/delete；`vector_store.rs` 是标注「适用于测试和小规模」的实现却被 `FacetStore` 依赖，且其 `add` 不回收、`remove` 不缩容（内存单调增长）、`search` 完全忽略 `SearchFilter`。
- **文档漂移**：`queue/mod.rs` 写「双队列为空时阻塞等待」而实现是 10ms 轮询；`facet/mod.rs:7` 引用不存在的 `retriever` 模块；`client/connection.rs:48` 注释写 1MB 而实现是 64MB（且 CLI 用 `>=`、MCP 用 `>`）；`memory_write.rs` 注释称 `min_score` 由模型默认值覆盖，实际 `ModelType::default_min_score` 是死代码。

## 六、修复建议排序

| 批次 | 内容 | 理由 |
|---|---|---|
| **第 1 批（P0，必须）** | P0-1 socket 分帧读取、P0-2 连接超时与并发上限、P0-4 模型默认值、P0-5 TTL 上界、P0-6 MCP 错误判定 | 直接影响可用性与数据正确性，改动局部、可独立验证；P0-1/6 决定 MCP 与 DSH 插件能否写入大正文 |
| **第 2 批（P0 数据一致性）** | P0-7/P0-8 Dream 锁（create_new + RAII guard）、P0-9 解析失败不回退写库、P0-3 `.mr_pid` 原子创建 | 涉及 Dream 与项目归属，需配套故障注入测试 |
| **第 3 批（P1 隔离与误判）** | P1-1 图命中过滤、P1-2 读类工具错误、P1-3 include_global、P1-4 `.mr_pid` 降级、P1-5 跨项目去重 | 都是「返回错误结果却不报错」，静默性最强 |
| **第 4 批（P1 性能）** | P1-11 向量全量重写、P1-13 posix 阻塞、P1-9 全量装载、P1-21/P1-29 全量 list 与参数上界 | 需要基准或门禁数据支撑，改动面较大 |
| **第 5 批（P2）** | 死配置清理、未接线模块删除或接线、错误码收敛、文档漂移 | 低风险清理，可独立提交 |

## 七、结论

项目整体工程质量在同规模 Rust 项目中属中上：格式化与 clippy 双清零、无 unsafe 隐患、`cargo test` 16/16、检索门禁长期达标、文件拆分基本守规（无超 1000 行文件）。真正的风险集中在三点：

1. **静默降级成体系**：错误被 `.ok()`/`if let Ok`/`unwrap_or_default` 折叠成「成功但结果不对」，P0-6、P1-2、P1-14~P1-23 都是同一模式的不同表现。这是本项目最需要一次性治理的系统性问题，建议确立「降级必须留痕（warn + 指标）」的硬约定。
2. **异步上下文中的同步 I/O 与锁内 I/O 未治理**：P1-11~P1-13 会在库规模增长后直接体现为检索 p99 劣化。
3. **协议面大于实现面**：9 个无 handler 的 action、多处无消费方的配置、两套配置真相，使「看代码以为支持、运行时静默不支持」。

P0 共 9 条，全部已复核，建议按第 1、2 批顺序修复并各自配套回归测试；第 1 批的 P0-1 与 P0-6 直接决定 MCP 与 DSH 插件能否写入超过 8KB 的正文和能否正确报告写入失败，应优先处理。
