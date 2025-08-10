# Quarry

Quarry is a Rust library for mining type information from the Rust standard library.
It provides access to struct field information, including private fields, by analyzing
the actual standard library installed on your system.

**Note:** The API is currently unstable.

## Scope and Limitations

**Current Focus**: Quarry currently analyzes **structs** only. Popular types like `Option<T>` and `Result<T, E>` (which are enums) cannot be analyzed yet.

**Planned Features**: Support for enums, traits, and other types is planned for future releases. If you need enum analysis immediately, consider using `rustdoc` directly.

## Overview

Quarry dynamically analyzes the Rust standard library installed on your system to extract detailed information about structs, including:

- Field names and types (including private fields)
- Visibility (public/private)
- Struct type (named, tuple, or unit struct)
- Full module path resolution

## Requirements

To use the full functionality of Quarry, you need:

1. **Nightly Rust toolchain** (for rustdoc JSON generation):
   ```bash
   rustup toolchain install nightly
   ```

2. **Rust source code** (for analyzing the standard library):
   ```bash
   rustup component add rust-src --toolchain nightly
   ```

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
quarry = "0.1.0"
```

## Usage

### Basic Usage

```rust
use quarry::mine_struct_info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Analyze alloc::string::String (requires full module path)
    let result = mine_struct_info("alloc::string::String")?;
    
    println!("Struct: {}", result.name);
    println!("Simple name: {}", result.simple_name);
    println!("Module path: {}", result.module_path);
    
    // Print fields (including private fields)
    for field in result.fields {
        println!("  {} -> {} (public: {})", 
            field.name, field.type_name, field.is_public);
    }
    
    Ok(())
}
```

### Full Module Paths Required

Quarry requires explicit, full module paths to ensure unambiguous type resolution:

```rust
// ✅ Correct usage - full module paths
let string_info = mine_struct_info("alloc::string::String")?;
let vec_info = mine_struct_info("alloc::vec::Vec")?;
let hashmap_info = mine_struct_info("std::collections::HashMap")?;

// ❌ Incorrect usage - will fail
let result = mine_struct_info("String"); // Error: requires full path
let result = mine_struct_info("Vec");    // Error: requires full path
```

### Cache Management

Quarry caches the analyzed standard library information for performance:

```rust
use quarry::{init_stdlib_cache, cache_stats, clear_stdlib_cache};

// Initialize cache explicitly (optional)
init_stdlib_cache()?;

// Check cache statistics
let (count, initialized) = cache_stats()?;
println!("Cache has {} types, initialized: {}", count, initialized);

// Clear cache if needed
clear_stdlib_cache();
```

### Listing Available Types

```rust
use quarry::list_stdlib_structs;

// Get all available standard library struct types
let structs = list_stdlib_structs()?;
for struct_name in structs {
    println!("{}", struct_name);
}
```

### Checking Type Availability

```rust
use quarry::is_stdlib_struct;

// Quick check if a type exists in the standard library
if is_stdlib_struct("alloc::string::String") {
    println!("String is available in the standard library");
}
```

## Debugging and Logging

Quarry includes comprehensive debug logging throughout the analysis pipeline. This is especially useful for understanding what's happening during cache initialization, type lookup, and rustdoc generation.

### Enabling Debug Logs

Quarry uses the standard `log` crate for logging. To see debug output, add `env_logger` to your dependencies and initialize it:

```toml
[dependencies]
env_logger = "0.11"
```

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the logger to see debug output
    env_logger::init();
    
    // Your code here...
}
```

Then run your application with the `RUST_LOG` environment variable:

```bash
# See all debug logs
RUST_LOG=debug cargo run

# See only Quarry debug logs
RUST_LOG=quarry=debug cargo run

# See only standard library module logs
RUST_LOG=quarry::stdlib=debug cargo run
```

## Examples

Quarry includes comprehensive examples that demonstrate its capabilities:

### Basic Usage Example

```bash
cargo run --example basic_usage
```

Shows fundamental usage including:
- Simple struct analysis
- Field information extraction
- Error handling
- Basic output formatting

### Advanced Usage Example

```bash
cargo run --example advanced_usage
```

Demonstrates advanced features including:
- Cache management and statistics
- Bulk type analysis
- Type discovery and filtering
- Performance testing

Both examples can be run with debug logging:

```bash
RUST_LOG=quarry=debug cargo run --example basic_usage
RUST_LOG=quarry=debug cargo run --example advanced_usage
```

## How It Works

Quarry uses the following approach:

1. **Dynamic Analysis**: Uses `rustdoc` to analyze the actual standard library source code installed on your system
2. **Direct JSON Parsing**: Parses the generated `rustdoc` JSON output directly to extract struct information including private fields
3. **In-Memory Caching**: Stores the parsed struct information in a lookup table for fast subsequent queries
4. **Exact Path Matching**: Takes user input as exact module paths (e.g., "alloc::string::String") and looks them up directly in the cache

## Architecture

```
┌─────────────────┐
│   Your Code     │  mine_struct_info("alloc::string::String")
└─────────────────┘
          │
          ▼
┌─────────────────┐
│     Quarry      │  First call: run cargo doc → parse JSON → populate cache
│   (Public API)  │  Subsequent calls: direct lookup in cache
└─────────────────┘
          │
          ▼
┌─────────────────┐       ┌─────────────────┐
│  Memory Cache   │       │ Standard Library│
│   (HashMap)     │       │   Source Code   │
│ "alloc::string  │       │  (rust-src)     │
│ ::String" → ... │       └─────────────────┘
└─────────────────┘                │
                                   ▼
                          ┌─────────────────┐
                          │  cargo doc      │
                          │ --output-format │
                          │     json        │
                          └─────────────────┘
```

## Error Handling

Quarry provides detailed error information:

- `TypeNotFound`: The requested type was not found in the standard library
- `NotAStruct`: The requested type exists but is not a struct
- `StdlibAnalysis`: Failed to generate or parse rustdoc JSON (usually due to missing nightly toolchain or rust-src)
- `Io`: File system or process execution errors

## Limitations

- **Standard Library Only**: Currently only supports types from std, alloc, and core crates
- **Nightly Rust Required**: Requires nightly Rust toolchain for rustdoc JSON generation
- **rust-src Component Required**: Requires rust-src component for standard library analysis
- **Struct Types Only**: Currently focuses on struct types (enum and trait support will be added in future versions)
- **Performance**: Initial cache initialization depends on rustdoc generation speed

## Contributing

Contributions are welcome! Please feel free to submit an issue or a pull request.

## License

MIT License - see LICENSE file for details.
