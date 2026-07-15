# 模型切换与抽象方案

## 当前状态
- 当前模型：`Qdrant/all-MiniLM-L6-v2-onnx`
- 维度：384
- 文件：5个文件（model.onnx + tokenizer配置）
- 性能：~118MB内存，<1ms延迟

## BGE-M3 分析
- 模型：`BAAI/bge-m3`
- 维度：1024
- 特点：多语言支持，更好的语义表示
- 文件：需要检查是否有ONNX版本
- 大小：预计更大，可能有性能影响

## 抽象设计方案

### 1. 模型配置抽象
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelType {
    MiniLM6V2,
    BGEM3,
    Custom { name: String, dimension: usize, files: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_type: ModelType,
    pub source: String, // huggingface, local, etc
    pub dimension: usize,
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub filename: String,
    pub sha256: String,
    pub url_template: String, // {repo}/{filename}
}
```

### 2. 模型注册表
```rust
pub struct ModelRegistry {
    models: HashMap<String, ModelConfig>,
}

impl ModelRegistry {
    pub fn default() -> Self {
        let mut models = HashMap::new();
        
        models.insert("minilm-l6-v2".to_string(), ModelConfig {
            model_type: ModelType::MiniLM6V2,
            source: "huggingface".to_string(),
            dimension: 384,
            files: vec![
                // 当前5个文件
            ],
        });
        
        models.insert("bge-m3".to_string(), ModelConfig {
            model_type: ModelType::BGEM3,
            source: "huggingface".to_string(),
            dimension: 1024,
            files: vec![
                // BGE-M3文件（待确认）
            ],
        });
        
        Self { models }
    }
}
```

### 3. 嵌入生成器抽象
```rust
pub trait EmbeddingGenerator: Send + Sync {
    fn dimension(&self) -> usize;
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub struct FastEmbedGenerator {
    model_type: ModelType,
    inner: Mutex<TextEmbedding>,
}

impl EmbeddingGenerator for FastEmbedGenerator {
    fn dimension(&self) -> usize {
        match self.model_type {
            ModelType::MiniLM6V2 => 384,
            ModelType::BGEM3 => 1024,
            ModelType::Custom { ref dimension, .. } => *dimension,
        }
    }
    
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // 通用fastembed逻辑
    }
}
```

### 4. 模型选择机制
```toml
# config.toml
[embedding]
model = "bge-m3"  # 或 "minilm-l6-v2", "custom"
model_dir = "~/.memrec/models/bge-m3-onnx"
```

### 5. 迁移路径

#### 阶段1：抽象现有代码（向后兼容）
1. 创建`ModelType`枚举和`ModelConfig`
2. 重构`FastEmbedGenerator`支持多种模型
3. 更新`mr-install`支持模型选择
4. 保持默认`minilm-l6-v2`

#### 阶段2：添加BGE-M3支持
1. 确认BGE-M3 ONNX可用性
2. 添加模型配置和文件哈希
3. 测试性能和内存占用
4. 文档更新

#### 阶段3：模型切换功能
1. CLI命令切换模型：`memrec model switch bge-m3`
2. 自动下载和切换
3. 向量存储维度兼容性处理

## 技术挑战

### 1. 向量维度不兼容
- **问题**：384维 → 1024维，现有向量存储不兼容
- **解决方案**：
  - 切换时重建所有嵌入
  - 或：支持多模型向量存储（复杂）
  - 建议：重建嵌入，警告用户

### 2. 内存和性能
- **BGE-M3**：更大模型，更高内存占用
- **性能**：可能影响延迟
- **测试**：需要基准测试

### 3. 模型文件管理
- **多模型支持**：`~/.memrec/models/`下多目录
- **切换**：环境变量或配置指向不同目录
- **下载**：`mr-install --model bge-m3`

### 4. 向后兼容
- **默认模型**：保持`minilm-l6-v2`
- **配置升级**：自动检测并迁移
- **CLI**：新增`model`子命令

## 实施步骤

### 第1周：抽象基础
1. 创建模型类型和配置系统
2. 重构嵌入生成器为通用接口
3. 更新配置文件格式
4. 测试向后兼容性

### 第2周：BGE-M3集成
1. 确认BGE-M3 ONNX文件
2. 添加模型配置和哈希
3. 实现模型下载支持
4. 性能基准测试

### 第3周：切换功能
1. CLI模型管理命令
2. 向量存储重建工具
3. 文档和用户指南
4. 发布v0.4.0

## 风险评估

### 低风险
- 抽象现有代码（保持兼容）
- 配置扩展（添加字段）

### 中风险
- BGE-M3性能未知
- 向量存储重建可能耗时

### 高风险
- 向量维度变更破坏现有数据
- 内存占用增加

## 建议

1. **先进行抽象**：不急于切换到BGE-M3
2. **性能测试**：小规模测试BGE-M3后再决定
3. **用户选项**：提供选择，保持默认
4. **数据迁移**：提供工具和文档

## 结论

**可行**，但建议分阶段实施：
1. ✅ 先抽象支持多模型
2. ⏳ 评估BGE-M3的实际效果
3. ⚙️ 谨慎实施切换功能