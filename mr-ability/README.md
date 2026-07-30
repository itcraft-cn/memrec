# mr-ability

Core ability library for MemRec - provides memory storage, semantic search, project isolation, and other foundational services.

## Overview

`mr-ability` is the heart of the MemRec ecosystem, implementing the core capabilities that enable AI memory persistence across sessions. It provides:

- **Multi-layer Storage**: RocksDB-based persistent storage with vector, graph, and full-text capabilities
- **Semantic Search**: Hybrid search combining vector similarity, BM25, and graph traversal
- **Memory Compression**: Automatic consolidation of old memories through Dream processing
- **Project Isolation**: Workspace-based memory separation via `.mr_pid` files
- **Embedding Generation**: Text-to-vector conversion for semantic search

## Architecture

```
mr-ability/
├── storage/      # Multi-layer storage (RocksDB + Tantivy + Vector)
├── search/       # Search algorithms (MMR, scoring)
├── embedding/    # Text embedding generation
├── dream/        # Memory consolidation
├── project/      # Project detection and isolation
├── tiered/       # Tiered search system
├── importance/   # Memory importance evaluation
├── lifecycle/    # Memory lifecycle management
├── facet/        # Faceted search
├── edge/         # Graph edge management
├── propagation/  # Memory propagation
├── archive/      # Memory archiving
└── rule/         # Rule engine
```

## Core Components

### Storage Layer

The storage layer provides three dimensions of data persistence:

| Store | Description | Backend |
|-------|-------------|---------|
| `MemoryStore` | Memory CRUD operations | RocksDB |
| `VectorStore` | Embedding storage & search | RocksDB |
| `EdgeStore` | Graph edges & traversal | RocksDB |
| `TantivyStore` | Full-text search | Tantivy |
| `HybridStore` | Combined vector + BM25 | Vector + Tantivy |

### Search System

Supports multiple search strategies:

```rust
use mr_ability::search::{mmr_rerank, MmrConfig};

// MMR re-ranking for diversity
let hits = mmr_rerank(&candidates, query_embedding, MmrConfig {
    lambda: 0.5,
    top_k: 10,
});
```

### Embedding Generation

```rust
use mr_ability::embedding::{EmbeddingGenerator, GeneratorFactory};

let generator = GeneratorFactory::create(config)?;
let embedding = generator.embed("Important decision: Use Rust")?;
```

### Dream Processing

```rust
use mr_ability::dream::{DreamProcessor, DreamGate};

// Check if Dream should run
let gate = DreamGate::new(storage, config);
if gate.should_run()? {
    let processor = DreamProcessor::new(llm_client);
    let result = processor.consolidate(old_memories)?;
}
```

### Project Detection

```rust
use mr_ability::project::detect_project_id;

// Automatically detect project from current directory
let project_id = detect_project_id("/path/to/workspace")?;
```

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
mr-ability = { path = "../mr-ability" }
```

Basic example:

```rust
use mr_ability::storage::{MemoryStore, RocksDBStore};
use mr_common::Memory;

// Initialize storage
let store = RocksDBStore::open("/path/to/db")?;
let memory_store = MemoryStore::new(store);

// Add memory
let memory = Memory::new(
    "Important decision".to_string(),
    MemoryType::Decision,
);
let id = memory_store.add(&memory)?;

// Search memories
let hits = memory_store.search("decision", 10)?;
```

## Storage Schema

The storage uses RocksDB column families to organize data:

| Column Family | Content |
|---------------|---------|
| `memories` | Memory records (JSON) |
| `vectors` | Embedding vectors (f32 array) |
| `edges` | Graph edges (source → target) |
| `facets` | Metadata facets |
| `rules` | Automation rules |
| `config` | Configuration KV pairs |

## Performance

- **Vector Search**: ~10ms for 100K vectors (HNSW-like performance)
- **Hybrid Search**: ~50ms combining vector + BM25 + graph
- **Memory Add**: ~5ms including embedding generation
- **Dream**: ~2s for consolidating 10 memories (LLM-dependent)

## Dependencies

- **RocksDB**: Persistent key-value storage
- **Tantivy**: Full-text search engine
- **FastEmbed**: ONNX-based embedding inference
- **SQLite**: Metadata indexing
- **Tokio**: Async runtime

## License

Apache-2.0