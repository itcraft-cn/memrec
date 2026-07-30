# 系统空闲时自动触发Dream融通作业设计

## 概述

为MemRec守护进程增加系统空闲检测能力，在系统负载较低时自动触发Dream融通作业，实现跨项目记忆整合、个人记忆提炼、记忆清理和向量重新生成。

## 目标

1. 系统空闲时自动触发Dream作业（可配置）
2. 支持手动触发Dream作业
3. 融通作业与正常请求并行，但不影响用户体验
4. 分批处理+间隔休息，控制资源使用
5. 日志按天滚存，可配置保留天数

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────┐
│                    memrecd 守护进程                   │
│                                                     │
│  ┌─────────────────┐     ┌─────────────────┐       │
│  │  Normal Queue   │     │   Dream Queue   │       │
│  │    (FIFO)       │     │    (FIFO)       │       │
│  │                 │     │                 │       │
│  │  [A][B][C]...   │     │  [Dream1]...    │       │
│  └────────┬────────┘     └────────┬────────┘       │
│           │                       │                 │
│           │ 优先级：高            │ 优先级：低      │
│           │                       │                 │
│           └───────────┬───────────┘                 │
│                       ↓                             │
│            ┌─────────────────────┐                  │
│            │   Queue Dispatcher  │                  │
│            │                     │                  │
│            │  if !normal.empty() │                  │
│            │    → 取normal       │                  │
│            │  else               │                  │
│            │    → 取dream        │                  │
│            └─────────────────────┘                  │
│                                                     │
│  ┌──────────────────────────────────────────────┐  │
│  │          SystemIdleMonitor                   │  │
│  │  检测空闲 → 插入Dream到Dream Queue           │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 核心组件

1. **SystemIdleMonitor**: 空闲检测器，定时检查系统负载
2. **QueueDispatcher**: 队列调度器，优先处理普通队列
3. **DreamProcessor**: 融通处理器，执行4个阶段任务
4. **LogRotation**: 日志滚存，按天滚存并清理旧日志

### 请求处理流程

```
用户请求 → Normal Queue (FIFO)
系统空闲 → Dream Queue (FIFO)

调度策略：
if !normal_queue.is_empty() {
    request = normal_queue.pop_front()
} else if !dream_queue.is_empty() {
    request = dream_queue.pop_front()
}

execute(request)
```

## 详细设计

### 1. SystemIdleMonitor

**职责**：定时检查系统负载，检测到空闲时向Dream Queue发送请求

**配置**：

```toml
[idle_monitor]
enabled = true
check_interval_secs = 300      # 5分钟检查一次
load_threshold = 0.5           # load average < 0.5 视为空闲
auto_dream_enabled = true      # 自动触发dream
```

**实现逻辑**：

```rust
pub struct SystemIdleMonitor {
    config: IdleMonitorConfig,
    sender: mpsc::Sender<QueuedRequest>,
}

impl SystemIdleMonitor {
    pub async fn run(&self) {
        let mut interval = interval(Duration::from_secs(self.config.check_interval_secs));
        
        loop {
            interval.tick().await;
            
            if !self.config.enabled || !self.config.auto_dream_enabled {
                continue;
            }
            
            let load = get_system_load_1min();
            
            if load < self.config.load_threshold {
                info!("IdleMonitor: system load {} < threshold {}, triggering dream", 
                      load, self.config.load_threshold);
                self.sender.send(QueuedRequest::Dream).await;
            }
        }
    }
}

fn get_system_load_1min() -> f64 {
    #[cfg(target_os = "linux")]
    {
        let content = fs::read_to_string("/proc/loadavg").unwrap();
        content.split_whitespace().next().unwrap().parse().unwrap()
    }
    
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("sysctl").arg("-n").arg("vm.loadavg").output().unwrap();
        let content = String::from_utf8_lossy(&output.stdout);
        let load_str = content.trim().replace("{ ", "").replace(" }", "");
        load_str.split_whitespace().next().unwrap().parse().unwrap()
    }
}
```

### 2. QueueDispatcher

**职责**：从两个队列中按优先级取出请求，串行执行

**配置**：

```toml
[queue]
normal_queue_capacity = 1000
dream_queue_capacity = 10
```

**实现逻辑**：

```rust
pub struct QueueDispatcher {
    normal_queue: VecDeque<QueuedRequest>,
    dream_queue: VecDeque<QueuedRequest>,
    handler: RequestHandler,
}

impl QueueDispatcher {
    pub async fn run(&mut self) {
        loop {
            let request = if !self.normal_queue.is_empty() {
                self.normal_queue.pop_front()
            } else if !self.dream_queue.is_empty() {
                self.dream_queue.pop_front()
            } else {
                // 队列为空，等待新请求
                self.wait_for_request().await;
                continue;
            };
            
            if let Some(req) = request {
                self.execute(req).await;
            }
        }
    }
    
    pub fn push_normal(&mut self, request: QueuedRequest) {
        self.normal_queue.push_back(request);
    }
    
    pub fn push_dream(&mut self, request: QueuedRequest) {
        self.dream_queue.push_back(request);
    }
}
```

### 3. DreamProcessor增强

**职责**：执行4个阶段的融通任务

**配置**：

```toml
[dream]
# 现有配置
enabled = true
min_memories = 20
max_age_hours = 168
min_hours_between = 24.0

# 新增：阶段开关
phase_cross_project = true
phase_personal_summary = true
phase_cleanup = true
phase_vector_regen = false  # 资源消耗大，默认关闭

# 新增：批处理配置
batch_size = 100
batch_interval_ms = 1000
```

**4个阶段**：

#### Phase 1: 交叉项目记忆提取

```
1. 获取所有项目ID列表
2. 按项目分组统计记忆类型/标签分布
3. 找出跨项目共性的标签/主题
4. 提取共性记忆，生成交叉项目知识（scope: Global）
5. 分批处理，每批休息
```

#### Phase 2: 个人总体记忆提炼

```
1. 获取所有global scope的记忆
2. 按memory_type分组
3. 统计偏好模式
4. 生成个人总体记忆（不调用LLM，简单聚合）
```

#### Phase 3: 记忆清理

```
1. 识别过期记忆（超过N天未访问、importance低于阈值）
2. 执行清理策略（软删除或硬删除）
3. 清理向量存储中对应的向量
4. 分批处理，每批休息
```

#### Phase 4: 向量重新生成

```
1. 扫描所有记忆
2. 检查向量是否存在
3. 为缺失的记忆生成向量
4. 分批处理，每批休息
5. 持久化向量存储
```

**实现框架**：

```rust
pub struct DreamProcessor {
    config: DreamConfig,
    lock: DreamLock,
    storage: Arc<dyn MemoryStorage>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<dyn EmbeddingGenerator>,
}

impl DreamProcessor {
    pub async fn execute(&self, force: bool) -> Result<DreamResult> {
        // 门控检查
        if !force {
            self.check_gate()?;
        }
        
        // 获取锁
        self.lock.try_acquire()?;
        
        let mut results = Vec::new();
        
        // Phase 1: 交叉项目记忆提取
        if self.config.phase_cross_project {
            results.push(self.cross_project_extract().await);
        }
        
        // Phase 2: 个人总体记忆提炼
        if self.config.phase_personal_summary {
            results.push(self.personal_summary().await);
        }
        
        // Phase 3: 记忆清理
        if self.config.phase_cleanup {
            results.push(self.memory_cleanup().await);
        }
        
        // Phase 4: 向量重新生成
        if self.config.phase_vector_regen {
            results.push(self.vector_regenerate().await);
        }
        
        // 释放锁
        self.lock.release()?;
        
        Ok(DreamResult { phase_results: results, ... })
    }
    
    async fn cross_project_extract(&self) -> PhaseResult {
        let memories = self.storage.list_all_projects().await?;
        
        for batch in memories.chunks(self.config.batch_size) {
            self.process_batch(batch)?;
            sleep(Duration::from_millis(self.config.batch_interval_ms)).await;
        }
        
        PhaseResult { phase: CrossProjectExtract, success: true, ... }
    }
    
    // 其他阶段类似...
}
```

### 4. 日志滚存

**配置**：

```toml
[log]
enabled = true
level = "info"
rotation_days = 7
compress = false

[log.files]
memrecd = "~/.memrec/logs/memrecd.log"
dream = "~/.memrec/logs/dream.log"
```

**日志文件结构**：

```
~/.memrec/logs/
├── memrecd.log                    # 主日志
├── memrecd-2026-07-30.log.gz
├── ...
│
├── dream.log                      # dream作业独立日志
├── dream-2026-07-30.log.gz
└── ...
```

**实现方式**：

使用 `tracing-appender` crate：

```rust
pub fn init_log(config: &LogConfig) -> Result<()> {
    let logs_dir = config.files.memrecd.parent().unwrap();
    create_dir_all(logs_dir)?;
    clean_old_logs(logs_dir, config.rotation_days)?;
    
    let memrecd_appender = RollingFileAppender::new(
        Rotation::DAILY,
        logs_dir,
        "memrecd.log",
    );
    
    let dream_appender = RollingFileAppender::new(
        Rotation::DAILY,
        logs_dir,
        "dream.log",
    );
    
    let (memrecd_nb, _guard1) = tracing_appender::non_blocking(memrecd_appender);
    let (dream_nb, _guard2) = tracing_appender::non_blocking(dream_appender);
    
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(memrecd_nb)
                .with_filter(filter::filter_fn(|meta| {
                    meta.target() != "dream"
                }))
        )
        .with(
            fmt::layer()
                .with_writer(dream_nb)
                .with_filter(filter::filter_fn(|meta| {
                    meta.target() == "dream"
                }))
        )
        .init();
    
    Ok(())
}

fn clean_old_logs(dir: &Path, keep_days: u64) -> Result<()> {
    let cutoff = Utc::now() - chrono::Duration::days(keep_days as i64);
    let cutoff_secs = cutoff.timestamp() as u64;
    
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        
        let file_secs = metadata.modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        if file_secs < cutoff_secs {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
```

**使用方式**：

```rust
// 普通日志 → memrecd.log
info!("Processing search request");

// Dream日志 → dream.log
tracing::info!(target: "dream", "Dream Phase 1 started");
tracing::info!(target: "dream", "Dream Phase 1 completed: 150 memories");
```

### 5. 命令行接口

**新增命令**：

```bash
# 手动触发dream（已有）
memrec dream --force

# 查看队列状态（新增）
memrec queue status

# 手动触发空闲检测（新增）
memrec idle check

# 查看dream历史（新增）
memrec dream history --limit 10
```

**CLI参数扩展**：

```rust
enum Command {
    Add(AddArgs),
    Search(SearchArgs),
    Dream(DreamArgs),
    Queue(QueueArgs),    // 新增
    Idle(IdleArgs),      // 新增
}

pub struct DreamArgs {
    pub force: bool,
    
    #[arg(long)]
    pub phase: Option<String>,
    
    #[arg(long)]
    pub only_cleanup: bool,
}
```

### 6. 错误处理

**三个层级**：

1. **阶段内错误（可恢复）**：单批次失败 → 跳过，继续下一批
2. **阶段错误（可跳过）**：Phase失败 → 记录日志，继续下一阶段
3. **致命错误（需停止）**：存储连接失败 → 返回错误，不继续

**日志输出示例**：

```
2026-07-30 10:00:00 INFO  IdleMonitor: system load 0.25 < threshold 0.5, triggering dream
2026-07-30 10:00:01 INFO  [dream] Dream Phase 1 started: CrossProjectExtract
2026-07-30 10:00:02 DEBUG [dream] Batch 1/10: processing 100 memories
2026-07-30 10:00:05 INFO  [dream] Dream Phase 1 completed: processed 150 memories, created 2
2026-07-30 10:00:06 INFO  [dream] Dream Phase 2 started: PersonalSummary
...
```

## 文件变更清单

```
mr-common/src/types/config.rs
├─ 新增 IdleMonitorConfig
├─ 新增 QueueConfig
├─ 新增 LogConfig
└─ DreamConfig 新增阶段开关和批处理配置

mr-ability/src/
├─ idle/
│   ├─ mod.rs              # 新增模块
│   └─ monitor.rs          # SystemIdleMonitor实现
│
├─ queue/
│   ├─ mod.rs              # 新增模块
│   ├─ dispatcher.rs       # QueueDispatcher实现
│   └─ types.rs            # QueuedRequest等类型
│
└─ dream/
    ├─ processor.rs        # 修改：增加4个阶段逻辑
    ├─ phases/
    │   ├─ mod.rs          # 新增
    │   ├─ cross_project.rs
    │   ├─ personal_summary.rs
    │   ├─ cleanup.rs
    │   └─ vector_regen.rs
    └─ lock.rs             # 不变

memrecd/src/
├─ daemon/mod.rs           # 修改：启动IdleMonitor和QueueDispatcher
├─ log/
│   ├─ mod.rs              # 新增模块
│   └─ rotation.rs         # 日志滚存和清理逻辑
├─ server/
│   ├─ mod.rs              # 修改：请求入队而非直接处理
│   └─ handler.rs          # 修改：适配队列模式
└─ config.rs               # 修改：加载新配置

memrec/src/
├─ main.rs                 # 修改：新增queue和idle命令
└─ commands/
    ├─ mod.rs              # 修改：导出新命令
    ├─ memory.rs           # 修改：dream命令扩展参数
    ├─ queue.rs            # 新增
    └─ idle.rs             # 新增
```

## 成功标准

1. 系统空闲时（load average < 阈值）自动触发Dream作业
2. 手动触发Dream作业仍可用
3. Dream执行期间，正常用户请求优先处理
4. 日志按天滚存，超过保留天数自动清理
5. 分批处理+间隔休息，不影响系统性能
6. 各阶段可独立开关配置

## 注意事项

1. LLM整合功能暂时略过，使用简单聚合逻辑
2. 向量重新生成阶段资源消耗大，默认关闭
3. 日志滚存依赖tracing-appender crate
4. 系统负载读取需兼容Linux和macOS