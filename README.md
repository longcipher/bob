# Rust Workspace Template

A modern Rust workspace template with Leptos CSR support, comprehensive linting, and best practices.

## Features

- **Workspace Layout**: Organized `bin/` and `crates/` structure
- **Leptos CSR**: Client-side rendered Leptos application template
- **Comprehensive Linting**: Strict clippy configuration with pedantic lints
- **Modern Tooling**: cargo-leptos, leptosfmt, taplo, typos, rumdl
- **Tailwind CSS**: Ready-to-use Tailwind configuration

## Project Structure

```text
rust-workspace-template/
├── Cargo.toml              # Root workspace manifest
├── Justfile                # Task runner commands
├── rust-toolchain.toml     # Rust version and components
├── .rustfmt.toml           # Rustfmt configuration
├── leptosfmt.toml          # Leptos formatter configuration
├── .taplo.toml             # TOML formatter configuration
├── .typos.toml             # Typo checker configuration
├── .rumdl.toml             # Markdown linter configuration
├── bin/
│   └── leptos-csr-app/     # Leptos CSR application
│       ├── Cargo.toml
│       ├── index.html
│       ├── src/
│       │   ├── main.rs
│       │   └── app.rs
│       ├── style/
│       │   ├── input.css
│       │   └── main.css
│       └── assets/
└── crates/
    └── common/             # Shared utilities
        ├── Cargo.toml
        └── src/
            └── lib.rs
```

## Prerequisites

Install required tools:

```bash
just setup
```

Or manually:

```bash
cargo install cargo-leptos
cargo install cargo-machete
cargo install taplo-cli
cargo install typos-cli
cargo install leptosfmt
```

## Quick Start

### Development

```bash
# Start frontend development server with hot reload
just fe-dev

# Or using cargo-leptos directly
cargo leptos watch -p leptos-csr-app
```

### Build

```bash
# Build entire workspace
just build

# Build frontend in release mode
just fe-build

# Build frontend in debug mode
just fe-build-dev
```

### Linting

```bash
# Run all lints
just lint

# Format code
just format

# Auto-fix issues
just fix
```

### Testing

```bash
# Run tests
just test

# Run tests with coverage
just test-coverage
```

### Maintenance

```bash
# Clean build artifacts
just clean

# Install required tools
just setup

# Serve the built frontend
just fe-serve
```

## Workspace Configuration

### Linting Philosophy

The workspace uses strict clippy lints with the following principles:

1. **Pedantic by default**: Enable all pedantic lints, then allow specific ones that are too noisy
2. **Panic safety**: Deny `unwrap`, `expect`, and `panic` - use proper error handling
3. **No debug code**: Deny `dbg!`, `todo!`, and `unimplemented!`
4. **No stdout in libraries**: Use `tracing` instead of `println!`/`eprintln!`

### Adding Dependencies

Always use `cargo add`:

```bash
# Add to workspace
cargo add <crate> --workspace

# Add to specific crate
cargo add <crate> -p <crate-name>
```

### Creating New Crates

```bash
# New binary
cargo new bin/my-app

# New library
cargo new --lib crates/my-lib
```

Then add `[lints] workspace = true` to the new crate's `Cargo.toml`.

## License

MIT
