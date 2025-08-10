//! # Quarry
//!
//! Quarry is a Rust library for mining type information from the Rust standard library.
//! It provides access to struct field information, including private fields, by analyzing
//! the actual standard library installed on your system.
//!
//! ## Scope and Limitations
//!
//! **Current Focus**: Quarry currently analyzes **structs** only. Popular types like `Option<T>` 
//! and `Result<T, E>` (which are enums) cannot be analyzed yet.
//!
//! **Planned Features**: Support for enums, traits, and other types is planned for future releases. 
//! If you need enum analysis immediately, consider using `rustdoc` directly.
//!
//! ## Requirements
//!
//! - **Nightly Rust Toolchain**: Required for rustdoc JSON generation
//! - **rust-src Component**: Install with `rustup component add rust-src --toolchain nightly`
//!
//! ## Usage Philosophy
//!
//! Quarry requires explicit, full module paths to ensure unambiguous type resolution.
//! Instead of accepting short names like "String", you must specify "alloc::string::String".
//! This design choice eliminates ambiguity and makes your code more explicit about which
//! specific type you're analyzing.
//!
//! ## Example
//!
//! ```rust,no_run
//! use quarry::mine_struct_info;
//!
//! // Analyze the String type from the alloc crate
//! let result = mine_struct_info("alloc::string::String")?;
//! println!("Struct: {}", result.name);
//! println!("Simple name: {}", result.simple_name);
//! println!("Module path: {}", result.module_path);
//!
//! // Access field information (including private fields)
//! for field in result.fields {
//!     println!("  Field: {} -> {} (public: {})",
//!              field.name, field.type_name, field.is_public);
//! }
//!
//! // List all available types
//! let all_types = quarry::list_stdlib_structs()?;
//! println!("Found {} standard library struct types", all_types.len());
//! # Ok::<(), quarry::QuarryError>(())
//! ```
//!
//! ## Debug Logging
//!
//! Quarry provides detailed debug logging to help understand the standard library analysis process.
//! To enable debug output:
//!
//! 1. **Add a logger to your Cargo.toml**:
//!    ```toml
//!    [dependencies]
//!    env_logger = "0.11"
//!    ```
//!
//! 2. **Initialize the logger in your code**:
//!    ```rust,no_run
//!    fn main() {
//!        env_logger::init();
//!        // ... your code using quarry
//!    }
//!    ```
//!
//! 3. **Run with debug environment variables**:
//!    - `RUST_LOG=debug` - Show all debug messages
//!    - `RUST_LOG=quarry=debug` - Show only Quarry debug messages
//!    - `RUST_LOG=quarry::stdlib=debug` - Show only stdlib module debug messages
//!
//! Example: `RUST_LOG=quarry=debug cargo run`

use log::debug;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod stdlib;

/// Errors that can occur when mining standard library type information
#[derive(Debug, Error)]
pub enum QuarryError {
    #[error("Type not found: {0}")]
    TypeNotFound(String),

    #[error("Type is not a struct: {0}")]
    NotAStruct(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Standard library analysis failed: {0}")]
    StdlibAnalysis(String),
}

pub type Result<T> = std::result::Result<T, QuarryError>;

/// Complete information about a struct
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructInfo {
    /// The full name of the struct (e.g., "std::string::String")
    pub name: String,
    /// The simple name without module path (e.g., "String")
    pub simple_name: String,
    /// The module path (e.g., "std::string")
    pub module_path: String,
    /// List of fields in the struct
    pub fields: Vec<FieldInfo>,
    /// Whether the struct is a tuple struct
    pub is_tuple_struct: bool,
    /// Whether the struct is a unit struct
    pub is_unit_struct: bool,
}

/// Information about a struct field
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldInfo {
    /// The name of the field
    pub name: String,
    /// The type of the field as a string
    pub type_name: String,
    /// Whether the field is public
    pub is_public: bool,
    /// The name of the struct this field belongs to
    pub struct_name: String,
}

impl StructInfo {
    /// Create a new StructInfo with the given name and extract module path components
    pub fn new(name: &str) -> Self {
        let (module_path, simple_name) = if let Some(pos) = name.rfind("::") {
            (name[..pos].to_string(), name[pos + 2..].to_string())
        } else {
            (String::new(), name.to_string())
        };

        Self {
            name: name.to_string(),
            simple_name,
            module_path,
            fields: Vec::new(),
            is_tuple_struct: false,
            is_unit_struct: false,
        }
    }
}

/// Mine struct information from the Rust standard library
///
/// This function queries the standard library cache for information about a specific struct.
/// It requires the full module path to ensure unambiguous type resolution (e.g.,
/// "alloc::string::String" rather than just "String").
///
/// # Arguments
///
/// * `name` - The full module path of the struct (e.g., "alloc::string::String")
///
/// # Examples
///
/// ```rust,no_run
/// use quarry::mine_struct_info;
///
/// // Standard library struct with full path
/// let string_info = mine_struct_info("alloc::string::String")?;
/// println!("Struct: {}", string_info.name);
/// println!("Fields: {}", string_info.fields.len());
///
/// // Vec from alloc crate
/// let vec_info = mine_struct_info("alloc::vec::Vec")?;
/// println!("Is tuple struct: {}", vec_info.is_tuple_struct);
///
/// // HashMap from std collections
/// let map_info = mine_struct_info("std::collections::HashMap")?;
/// for field in &map_info.fields {
///     println!("  Field: {} -> {}", field.name, field.type_name);
/// }
/// # Ok::<(), quarry::QuarryError>(())
/// ```
///
/// # Errors
///
/// Returns `QuarryError::TypeNotFound` if the specified struct is not found in the
/// standard library cache. Make sure you're using the complete module path.
pub fn mine_struct_info(name: &str) -> Result<StructInfo> {
    debug!("Mining struct information for: '{}'", name);

    match stdlib::mine_stdlib_struct_info(name) {
        Ok(info) => {
            debug!(
                "Successfully found '{}' with {} fields",
                name,
                info.fields.len()
            );
            Ok(info)
        }
        Err(e) => {
            debug!("Failed to find struct '{}': {:?}", name, e);
            Err(e)
        }
    }
}

/// Initialize the standard library cache
///
/// This function forces initialization of the standard library type cache.
/// Normally, the cache is initialized lazily on first use, but this can be
/// called explicitly if you want to handle any initialization errors upfront
/// or warm up the cache for better performance.
///
/// The initialization process analyzes the actual standard library installed
/// on your system using rustdoc JSON generation, which requires the nightly
/// toolchain and rust-src component.
///
/// # Examples
///
/// ```rust,no_run
/// use quarry::init_stdlib_cache;
///
/// // Initialize the cache upfront to handle any errors early
/// init_stdlib_cache()?;
///
/// // Now subsequent calls will be faster
/// let result = quarry::mine_struct_info("alloc::string::String")?;
/// # Ok::<(), quarry::QuarryError>(())
/// ```
///
/// # Errors
///
/// May return errors related to rustdoc JSON generation or standard library
/// analysis. Common issues include missing nightly toolchain or rust-src component.
pub fn init_stdlib_cache() -> Result<()> {
    debug!("Initializing standard library cache");

    // Force cache initialization by attempting to query a known type
    // We use alloc::string::String as it should always exist
    match stdlib::mine_stdlib_struct_info("alloc::string::String") {
        Ok(_) => {
            debug!("Standard library cache initialization completed successfully");
            Ok(())
        }
        Err(QuarryError::TypeNotFound(_)) => {
            // If String is not found, the cache was still initialized, just empty
            debug!("Cache initialized but String type not found (may be expected)");
            Ok(())
        }
        Err(e) => {
            debug!("Error during cache initialization: {:?}", e);
            Err(e)
        }
    }
}

/// Clear the standard library cache
///
/// This function clears the cached standard library type information.
/// The cache will be rebuilt on the next call to any function that requires it.
/// This can be useful for testing or if you want to refresh the cache
/// after updating your Rust installation.
///
/// # Examples
///
/// ```rust
/// use quarry::clear_stdlib_cache;
///
/// // Clear the cache to force rebuilding
/// clear_stdlib_cache();
///
/// // The next call will rebuild the cache from scratch
/// let result = quarry::mine_struct_info("alloc::string::String");
/// ```
pub fn clear_stdlib_cache() {
    debug!("Clearing standard library cache");
    stdlib::clear_cache();
    debug!("Standard library cache cleared");
}

/// Get statistics about the standard library cache
///
/// Returns a tuple of (number_of_cached_types, is_initialized).
///
/// # Examples
///
/// ```rust,no_run
/// use quarry::cache_stats;
///
/// let (count, initialized) = cache_stats()?;
/// println!("Cache contains {} types, initialized: {}", count, initialized);
/// # Ok::<(), quarry::QuarryError>(())
/// ```
pub fn cache_stats() -> Result<(usize, bool)> {
    stdlib::cache_stats()
}

/// List all available standard library struct types
///
/// Returns a sorted list of all struct types found in the standard library.
///
/// # Examples
///
/// ```rust,no_run
/// use quarry::list_stdlib_structs;
///
/// let structs = list_stdlib_structs()?;
/// for struct_name in structs.iter().take(10) {
///     println!("  {}", struct_name);
/// }
/// # Ok::<(), quarry::QuarryError>(())
/// ```
pub fn list_stdlib_structs() -> Result<Vec<String>> {
    stdlib::list_stdlib_structs()
}

/// Check if a type name refers to a standard library struct
///
/// This is a lightweight check that returns true if the given name
/// corresponds to a struct in the standard library. Requires the full
/// module path for accurate results.
///
/// # Examples
///
/// ```rust,no_run
/// use quarry::is_stdlib_struct;
///
/// // These will return true if the types exist in the standard library
/// assert!(is_stdlib_struct("alloc::string::String"));
/// assert!(is_stdlib_struct("alloc::vec::Vec"));
/// assert!(is_stdlib_struct("std::collections::HashMap"));
///
/// // These will return false
/// assert!(!is_stdlib_struct("MyCustomStruct"));
/// assert!(!is_stdlib_struct("some::external::Type"));
/// ```
///
/// # Performance
///
/// This is a fast lookup operation that checks the cache without
/// triggering expensive initialization if the cache is not ready.
pub fn is_stdlib_struct(name: &str) -> bool {
    stdlib::is_stdlib_struct(name)
}
