# bob-core

[![crates.io](https://img.shields.io/crates/v/bob-core.svg)](https://crates.io/crates/bob-core)
[![docs.rs](https://docs.rs/bob-core/badge.svg)](https://docs.rs/bob-core)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Core domain types and port traits for the [Bob Agent Framework](https://github.com/longcipher/bob).

## Overview

`bob-core` defines the hexagonal boundary of the Bob Agent Framework using the ports and adapters architecture pattern. This crate contains:

- **Domain Types**: Core data structures used throughout the framework
- **Port Traits**: Abstract interfaces that adapters must implement
- **Error Types**: Comprehensive error definitions for all components

This crate intentionally contains **no concrete implementations** — only contracts.

## Architecture

The crate defines four primary port traits:

1. **`LlmPort`** - Interface for language model interactions
2. **`ToolPort`** - Interface for tool/system operations
3. **`SessionStore`** - Interface for session state persistence
4. **`EventSink`** - Interface for event observation and logging

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
bob-core = "0.1"
```

### Example: Implementing a Custom LLM Adapter

```rust
use bob_core::{
    ports::LlmPort,
    types::{LlmRequest, LlmResponse, LlmStream},
    error::LlmError,
};
use async_trait::async_trait;

struct MyCustomLlm;

#[async_trait]
impl LlmPort for MyCustomLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Your implementation here
        todo!("implement LLM completion")
    }

    async fn complete_stream(&self, req: LlmRequest) -> Result<LlmStream, LlmError> {
        // Your implementation here
        todo!("implement streaming completion")
    }
}
```

## Features

- **Zero-cost abstractions**: All traits use `async_trait` for async support
- **Type-safe**: Strong typing throughout with comprehensive error handling
- **Serializable**: All domain types implement `serde::Serialize` and `Deserialize`
- **Thread-safe**: All traits require `Send + Sync`

## Documentation

Full API documentation is available at [docs.rs/bob-core](https://docs.rs/bob-core).

## Related Crates

- **[bob-runtime](https://crates.io/crates/bob-runtime)** - Runtime orchestration layer
- **[bob-adapters](https://crates.io/crates/bob-adapters)** - Concrete adapter implementations
- **[bob-cli](https://github.com/longcipher/bob)** - CLI application

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE.md) for details.
