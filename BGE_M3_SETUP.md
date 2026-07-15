# BGE-M3 设置指南

## 当前状态

BGE-M3 支持已集成到 MemRec 的抽象系统中，但**仍处于实验阶段**。

### 已完成
- ✅ 模型类型定义 (`ModelType::BGEM3`)
- ✅ 配置文件支持 (`model_type = "bge-m3"`)
- ✅ 文件哈希验证（部分文件）
- ✅ 抽象接口 (`BGEM3Generator`)

### 待完成
- ❌ 完整的文件哈希（需要下载大文件计算）
- ❌ fastembed 集成（BGE-M3使用不同的tokenizer格式）
- ❌ 性能测试和验证
- ❌ mr-install 自动下载支持

## 手动设置步骤

### 1. 下载 BGE-M3 模型文件

```bash
# 创建目录
mkdir -p ~/.memrec/models/BAAI--bge-m3/onnx
cd ~/.memrec/models/BAAI--bge-m3/onnx

# 下载文件（需要约 2.3GB 空间）
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/model.onnx" -o model.onnx
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/model.onnx_data" -o model.onnx_data
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/sentencepiece.bpe.model" -o sentencepiece.bpe.model
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/tokenizer.json" -o tokenizer.json
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/tokenizer_config.json" -o tokenizer_config.json
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/special_tokens_map.json" -o special_tokens_map.json
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/config.json" -o config.json
curl -L "https://hf-mirror.com/BAAI/bge-m3/resolve/main/onnx/Constant_7_attr__value" -o Constant_7_attr__value
```

### 2. 计算文件哈希

```bash
# 计算每个文件的SHA256哈希
sha256sum model.onnx
sha256sum model.onnx_data
sha256sum sentencepiece.bpe.model
sha256sum tokenizer.json
sha256sum tokenizer_config.json
sha256sum special_tokens_map.json
sha256sum config.json
sha256sum Constant_7_attr__value

# 更新 common/src/types/model.rs 中的 ModelFile::for_bge_m3() 函数
# 使用实际的哈希值替换 "0000..." 占位符
```

### 3. 更新配置文件

编辑 `~/.memrec/config.toml`：

```toml
version = "0.3.0-dev"

[model]
model_type = "bge-m3"  # 改为 bge-m3
source = "huggingface"
dimension = 1024

# 文件列表会自动从代码中读取
```

### 4. 重启守护进程

```bash
# 停止当前守护进程
systemctl --user stop memrecd  # 或 launchctl unload ...

# 启动新配置的守护进程
memrecd
```

## 技术挑战

### 1. Tokenizer 格式差异
- **MiniLML6V2**: 使用 `tokenizer.json` (HuggingFace 格式)
- **BGE-M3**: 使用 `sentencepiece.bpe.model` (SentencePiece 格式)
- **解决方案**: 需要修改 `fastembed` 使用或实现自定义 tokenizer

### 2. 大文件处理
- `model.onnx_data`: 2.27GB
- 内存占用可能显著增加
- 下载和验证时间较长

### 3. fastembed 兼容性
- `fastembed` 库可能不支持 BGE-M3 的 ONNX 格式
- 可能需要使用其他库（如 `onnxruntime`、`candle`）

## 临时解决方案

### 选项1：使用占位符实现
当前实现 (`BGEM3Generator`) 会抛出错误，提示功能未完成。用户可以：
1. 保持使用 MiniLML6V2（默认）
2. 等待完整实现

### 选项2：自定义实现
高级用户可以：
1. 实现 `EmbeddingGenerator` trait 用于 BGE-M3
2. 使用 `onnxruntime-rs` 加载 ONNX 模型
3. 使用 `sentencepiece` 库进行 tokenization

## 开发路线图

### 阶段1：基础支持（当前）
- [x] 抽象接口
- [x] 配置文件支持
- [ ] 完整文件哈希

### 阶段2：集成测试
- [ ] 下载脚本自动化
- [ ] 验证模型加载
- [ ] 性能基准测试

### 阶段3：生产就绪
- [ ] fastembed 兼容性修复
- [ ] mr-install 集成
- [ ] 文档和用户指南

## 已知问题

1. **哈希验证失败**: 大多数文件使用临时哈希 "0000..."
2. **tokenizer 不兼容**: fastembed 期望 HuggingFace 格式
3. **内存占用**: BGE-M3 需要更多内存（预计 300-500MB）
4. **性能影响**: 1024 维向量增加计算和存储开销

## 贡献指南

欢迎贡献 BGE-M3 支持！

1. **计算文件哈希**: 下载文件并提交正确的 SHA256 哈希
2. **修复 tokenizer**: 实现 SentencePiece 支持或转换工具
3. **性能优化**: 测试和优化 BGE-M3 的性能
4. **文档**: 更新安装和使用指南

## 紧急联系人

如有问题或需要帮助，请：
1. 查看 GitHub Issues: https://github.com/itcraft-cn/memrec/issues
2. 提交 Pull Request 改进实现
3. 在社区讨论技术方案

---

**注意**: BGE-M3 支持是实验性的，可能不稳定。建议生产环境继续使用 MiniLML6V2。