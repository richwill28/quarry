//! Dynamic analysis of Rust standard library types
//!
//! This module uses rustdoc JSON output to analyze the actual standard library
//! installed on the user's system and creates a lookup table for fast access.

use crate::{FieldInfo, QuarryError, Result, StructInfo};
use log::debug;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

// Constants for string parsing
const STD_SRC_PREFIX: &str = "std/src/";
const ALLOC_SRC_PREFIX: &str = "alloc/src/";
const CORE_SRC_PREFIX: &str = "core/src/";
const CRATE_PREFIX: &str = "crate::";

/// Global cache for standard library types
static STDLIB_CACHE: OnceLock<Mutex<Option<HashMap<String, StructInfo>>>> = OnceLock::new();

/// Initialize the standard library type database by analyzing the actual stdlib
fn init_stdlib_types() -> Result<HashMap<String, StructInfo>> {
    debug!("Initializing standard library type database");
    // Generate rustdoc JSON directly from the standard library source
    // This will include private fields when using --document-private-items
    let result = analyze_stdlib_with_rustdoc();
    match &result {
        Ok(types) => debug!(
            "Successfully initialized stdlib database with {} types",
            types.len()
        ),
        Err(e) => debug!("Failed to initialize stdlib database: {:?}", e),
    }
    result
}

/// Generate rustdoc JSON directly from the standard library
fn analyze_stdlib_with_rustdoc() -> Result<HashMap<String, StructInfo>> {
    debug!("Starting rustdoc analysis of standard library");

    // Find the standard library source
    debug!("Locating standard library source path");
    let stdlib_path = find_stdlib_source_path()?;
    debug!("Found stdlib source at: {:?}", stdlib_path);

    // Generate rustdoc JSON with private items included
    debug!("Generating rustdoc JSON for standard library");
    let types = generate_stdlib_rustdoc_json(&stdlib_path)?;
    debug!(
        "Generated and parsed {} types from rustdoc JSON",
        types.len()
    );

    Ok(types)
}

/// Find the path to the standard library source
fn find_stdlib_source_path() -> Result<std::path::PathBuf> {
    debug!("Finding standard library source path via nightly rustc");

    // Try to find the stdlib through nightly rustc (since we need nightly for rustdoc JSON)
    let output = std::process::Command::new("rustc")
        .args(&["+nightly", "--print", "sysroot"])
        .output()
        .map_err(QuarryError::Io)?;

    if !output.status.success() {
        debug!("Failed to get sysroot from nightly rustc");
        let error_msg = String::from_utf8_lossy(&output.stderr);
        debug!("Error output: {}", error_msg);
        return Err(QuarryError::TypeNotFound(
            "Could not find Rust nightly sysroot. Make sure nightly toolchain is installed with: rustup toolchain install nightly".to_string(),
        ));
    }

    let sysroot_string = String::from_utf8_lossy(&output.stdout);
    let sysroot = sysroot_string.trim();
    debug!("Found sysroot: {}", sysroot);

    let stdlib_path = std::path::PathBuf::from(sysroot)
        .join("lib")
        .join("rustlib")
        .join("src")
        .join("rust")
        .join("library")
        .join("std")
        .join("src");

    debug!("Checking for stdlib source at: {:?}", stdlib_path);
    if !stdlib_path.exists() {
        debug!("Standard library source not found at expected path");
        return Err(QuarryError::TypeNotFound(
            "Standard library source not found. Try installing rust-src component for nightly toolchain with: rustup component add rust-src --toolchain nightly".to_string()
        ));
    }

    debug!("Standard library source found successfully");
    Ok(stdlib_path)
}

/// Generate rustdoc JSON for the standard library with private items
fn generate_stdlib_rustdoc_json(
    stdlib_src_path: &std::path::Path,
) -> Result<HashMap<String, StructInfo>> {
    debug!(
        "Generating rustdoc JSON for stdlib at: {:?}",
        stdlib_src_path
    );

    // Navigate to the library workspace root where Cargo.toml is
    let library_root = stdlib_src_path.parent().ok_or_else(|| {
        QuarryError::TypeNotFound("Could not find library root directory".to_string())
    })?;

    debug!("Using library root directory: {:?}", library_root);

    // Check if Cargo.toml exists in the library root
    let cargo_toml_path = library_root.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        debug!("Cargo.toml not found at: {:?}", cargo_toml_path);
        return Err(QuarryError::TypeNotFound(
            "Standard library Cargo.toml not found. The rust-src component may be incomplete."
                .to_string(),
        ));
    }

    debug!("Found Cargo.toml at: {:?}", cargo_toml_path);

    // Create a temporary directory for the JSON output
    let temp_dir = std::env::temp_dir().join("quarry_stdlib_docs");
    debug!("Using temporary directory: {:?}", temp_dir);

    if temp_dir.exists() {
        debug!("Cleaning existing temporary directory");
        std::fs::remove_dir_all(&temp_dir).map_err(QuarryError::Io)?;
    }
    std::fs::create_dir_all(&temp_dir).map_err(QuarryError::Io)?;

    debug!("Executing cargo doc on the actual standard library workspace");

    // Use cargo doc with JSON output, but document multiple key crates
    let output = std::process::Command::new("cargo")
        .args(&[
            "+nightly",                 // Use nightly toolchain
            "doc",                      // Generate documentation
            "--package", "std",         // Document std package
            "--package", "alloc",       // Document alloc package
            "--package", "core",        // Document core package
            "--lib",                    // Document library only
            "--no-deps",                // Don't document dependencies
            "--document-private-items", // Include private items
            "--target-dir",
            temp_dir.to_str().unwrap(), // Custom target directory
        ])
        .env("RUSTDOCFLAGS", "-Z unstable-options --output-format json") // Enable JSON output
        .env("RUSTC_BOOTSTRAP", "1") // Allow unstable features
        .env("__CARGO_DEFAULT_LIB_METADATA", "stable") // Std library metadata
        .current_dir(library_root) // Run from library root
        .output()
        .map_err(QuarryError::Io)?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        debug!("Cargo doc command failed with error: {}", error_msg);

        // Log the stdout as well for debugging
        let stdout_msg = String::from_utf8_lossy(&output.stdout);
        if !stdout_msg.trim().is_empty() {
            debug!("Cargo doc stdout: {}", stdout_msg);
        }

        return Err(QuarryError::TypeNotFound(format!(
            "Failed to generate rustdoc JSON for standard library: {}",
            error_msg
        )));
    }

    debug!("Cargo doc execution completed successfully");

    // Find the generated JSON files
    let mut all_types = HashMap::new();

    // Check for std.json, alloc.json, and core.json
    let crate_names = ["std", "alloc", "core"];
    for crate_name in &crate_names {
        let json_path = temp_dir.join("doc").join(format!("{}.json", crate_name));
        debug!("Looking for {} JSON output at: {:?}", crate_name, json_path);

        if json_path.exists() {
            debug!("Found {} JSON at: {:?}", crate_name, json_path);
            // Parse this crate's JSON and merge into all_types
            let crate_types = parse_rustdoc_json_directly(&json_path)?;
            debug!(
                "Parsed {} types from {} crate",
                crate_types.len(),
                crate_name
            );

            // Merge the types
            for (name, struct_info) in crate_types {
                all_types.insert(name, struct_info);
            }
        } else {
            debug!("No JSON found for {} crate at: {:?}", crate_name, json_path);
        }
    }

    if all_types.is_empty() {
        debug!(
            "No types found after parsing all expected JSON files (std.json, alloc.json, core.json)"
        );
        return Err(QuarryError::TypeNotFound(format!(
            "Failed to parse any types from generated rustdoc JSON files"
        )));
    }

    debug!(
        "Successfully merged {} total types from all crates",
        all_types.len()
    );
    Ok(all_types)
}

/// Parse rustdoc JSON directly to extract struct information with private fields
fn parse_rustdoc_json_directly(json_path: &std::path::Path) -> Result<HashMap<String, StructInfo>> {
    debug!("Parsing rustdoc JSON from: {:?}", json_path);
    let mut types = HashMap::new();

    // Read and parse the JSON
    debug!("Reading JSON file content");
    let json_content = std::fs::read_to_string(json_path).map_err(QuarryError::Io)?;
    debug!("JSON file size: {} bytes", json_content.len());

    debug!("Parsing JSON content");
    let json: Value = serde_json::from_str(&json_content)
        .map_err(|e| QuarryError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    // Extract struct information from the JSON
    debug!("Looking for 'index' section in JSON");
    if let Some(index) = json.get("index") {
        if let Some(index_obj) = index.as_object() {
            debug!("Found index with {} items", index_obj.len());
            let mut processed = 0;

            for (_item_id, item_data) in index_obj {
                if let Some(struct_info) = parse_item_for_struct(item_data, &json)? {
                    debug!("Found struct: {}", struct_info.name);
                    // Insert with full name only - requires users to be explicit about paths
                    insert_struct_with_full_name(&mut types, struct_info);
                }
                processed += 1;
            }
            debug!(
                "Finished processing {} items, found {} structs",
                processed,
                types.len()
            );
        } else {
            debug!("Index section is not an object");
        }
    } else {
        debug!("No 'index' section found in JSON");
    }

    Ok(types)
}

/// Parse a single item from rustdoc JSON to see if it's a struct
///
/// This function examines a rustdoc JSON item and determines if it represents
/// a struct definition. It extracts the struct name, module path, fields, and
/// other metadata from the JSON structure.
///
/// # JSON Structure Example
///
/// For a struct like `String`, the JSON looks like:
/// ```json
/// {
///   "id": 246,
///   "crate_id": 0,
///   "name": "String",
///   "span": {
///     "filename": "alloc/src/string.rs",
///     "begin": [360, 1],
///     "end": [362, 2]
///   },
///   "visibility": "public",
///   "docs": "A UTF-8–encoded, growable string...",
///   "inner": {
///     "struct": {
///       "generics": { "params": [], "where_predicates": [] },
///       "kind": {
///         "plain": {
///           "fields": [5297]  // Field IDs to look up in the index
///         }
///       }
///     }
///   }
/// }
/// ```
///
/// # Returns
///
/// - `Ok(Some(StructInfo))` if the item is a struct
/// - `Ok(None)` if the item is not a struct or cannot be parsed
/// - `Err(QuarryError)` if there's an error during parsing
fn parse_item_for_struct(item_data: &Value, full_json: &Value) -> Result<Option<StructInfo>> {
    let item_obj = match item_data.as_object() {
        Some(obj) => obj,
        None => return Ok(None),
    };

    // Check if this item has struct inner data
    let inner = match item_obj.get("inner") {
        Some(inner) => inner,
        None => return Ok(None),
    };

    let inner_obj = match inner.as_object() {
        Some(obj) => obj,
        None => return Ok(None),
    };

    // Look for struct data
    let struct_data = match inner_obj.get("struct") {
        Some(data) => data,
        None => return Ok(None), // Not a struct
    };

    // Get the struct name
    let name = item_obj
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return Ok(None);
    }

    debug!("Parsing struct details for: {}", name);

    // Get the full path for this item
    debug!("Getting full path for struct: {}", name);
    let full_path = get_full_path_for_item(item_obj);
    let struct_name = if full_path.is_empty() {
        name.clone()
    } else {
        full_path
    };
    debug!("Full struct name: {}", struct_name);

    let mut struct_info = StructInfo::new(&struct_name);

    // Parse struct kind and fields
    debug!("Parsing struct kind and fields for: {}", struct_name);
    if let Some(struct_obj) = struct_data.as_object() {
        parse_struct_kind_and_fields(&mut struct_info, struct_obj, full_json)?;
        debug!(
            "Found {} fields for struct {}",
            struct_info.fields.len(),
            struct_name
        );
    }

    // Parse visibility for debugging
    if let Some(visibility) = item_obj.get("visibility") {
        debug!("Struct {} visibility: {:?}", struct_name, visibility);
    }

    Ok(Some(struct_info))
}

/// Get the full module path for an item
///
/// This function constructs the full module path for a Rust item by examining
/// its source file location and extracting the module hierarchy.
///
/// # JSON Structure Example
///
/// The function looks for span information in the JSON:
/// ```json
/// {
///   "name": "String",
///   "span": {
///     "filename": "alloc/src/string.rs",
///     "begin": [360, 1],
///     "end": [362, 2]
///   }
/// }
/// ```
///
/// From this, it extracts:
/// - `"alloc/src/string.rs"` → `"alloc::string"`
/// - Combined with name: `"alloc::string::String"`
///
/// # Supported Patterns
///
/// - `"std/src/collections/mod.rs"` → `"std::collections"`
/// - `"alloc/src/vec/mod.rs"` → `"alloc::vec"`
/// - `"core/src/ptr/mod.rs"` → `"core::ptr"`
///
/// # Arguments
///
/// * `item_obj` - The JSON object representing the item
///
/// # Returns
///
/// The full module path string, or just the item name if no path can be determined
fn get_full_path_for_item(item_obj: &serde_json::Map<String, Value>) -> String {
    let item_name = item_obj
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("unknown");

    debug!("Getting full path for item: {}", item_name);

    // Try to get the path from the item's span or other metadata
    if let Some(span) = item_obj.get("span") {
        debug!("Found span data for item: {}", item_name);
        if let Some(span_obj) = span.as_object() {
            if let Some(filename) = span_obj.get("filename") {
                if let Some(filename_str) = filename.as_str() {
                    debug!("Source filename for {}: {}", item_name, filename_str);
                    // Extract module path from filename
                    if let Some(module_path) = extract_module_path_from_filename(filename_str) {
                        let full_path = format!("{}::{}", module_path, item_name);
                        debug!("Constructed full path for {}: {}", item_name, full_path);
                        return full_path;
                    } else {
                        debug!(
                            "Could not extract module path from filename: {}",
                            filename_str
                        );
                    }
                }
            }
        }
    }

    // Fallback: just use the name
    debug!("Using fallback name for item: {}", item_name);
    item_name.to_string()
}

/// Extract module path from a source filename
///
/// This function parses Rust standard library source file paths and converts
/// them into module paths using Rust's module naming conventions.
///
/// # Examples
///
/// ```
/// // Standard library patterns:
/// extract_module_path_from_filename("std/src/collections/mod.rs")
///   // → Some("std::collections")
///
/// extract_module_path_from_filename("alloc/src/string.rs")
///   // → Some("alloc::string")
///
/// extract_module_path_from_filename("core/src/ptr/mod.rs")
///   // → Some("core::ptr")
///
/// // Non-standard library files:
/// extract_module_path_from_filename("src/main.rs")
///   // → None
/// ```
///
/// # Supported Crates
///
/// - **std**: `std/src/` → `std::`
/// - **alloc**: `alloc/src/` → `alloc::`  
/// - **core**: `core/src/` → `core::`
///
/// # Path Processing
///
/// The function filters out common Rust file patterns:
/// - `mod.rs` - Module definition files
/// - `lib.rs` - Library root files  
/// - `*.rs` - Individual source files
///
/// # Arguments
///
/// * `filename` - The source file path from rustdoc JSON
///
/// # Returns
///
/// * `Some(String)` - The module path if a recognized pattern is found
/// * `None` - If the file doesn't match any known standard library patterns

/// Helper function to process path parts by filtering out Rust file patterns
///
/// Takes a path string after the "src/" part and converts it into module path components.
/// Filters out special Rust files and strips .rs extensions.
///
/// # Arguments
///
/// * `path_after_src` - The portion of the path after "crate/src/"
///
/// # Returns
///
/// Vector of string slices representing module path components
///
/// # Examples
///
/// ```
/// process_path_parts("collections/hash_map.rs") // → ["collections", "hash_map"]
/// process_path_parts("string.rs")               // → ["string"]
/// process_path_parts("ptr/mod.rs")              // → ["ptr"]
/// process_path_parts("lib.rs")                  // → []
/// ```
fn process_path_parts(path_after_src: &str) -> Vec<&str> {
    path_after_src
        .split('/')
        .filter(|&part| part != "mod.rs" && part != "lib.rs")
        .map(|part| {
            if part.ends_with(".rs") {
                &part[..part.len() - 3]
            } else {
                part
            }
        })
        .collect()
}

fn extract_module_path_from_filename(filename: &str) -> Option<String> {
    debug!("Extracting module path from filename: {}", filename);

    // Look for std patterns - handle "std/src/" pattern
    if let Some(pos) = filename.find(STD_SRC_PREFIX) {
        debug!("Found std library pattern in filename at position: {}", pos);
        let after_src = &filename[pos + STD_SRC_PREFIX.len()..]; // Skip "std/src/"
        debug!("Path after 'std/src/': {}", after_src);

        let path_parts = process_path_parts(after_src);
        debug!("Filtered path parts: {:?}", path_parts);

        if !path_parts.is_empty() {
            // Handle special cases where public API differs from file structure
            let module_path = match path_parts.as_slice() {
                // Collections are exposed at std::collections level regardless of internal structure
                ["collections", "hash", "map"] => "std::collections".to_string(),
                ["collections", "hash", "set"] => "std::collections".to_string(),
                ["collections", "btree", "map"] => "std::collections".to_string(),
                ["collections", "btree", "set"] => "std::collections".to_string(),
                ["collections", "linked_list"] => "std::collections".to_string(),
                ["collections", "vec_deque"] => "std::collections".to_string(),
                ["collections", "binary_heap"] => "std::collections".to_string(),
                // For collections that are directly in collections/, use the first level
                parts if parts.len() >= 2 && parts[0] == "collections" => {
                    format!("std::collections")
                }
                // Default case: join all parts
                _ => format!("std::{}", path_parts.join("::")),
            };
            debug!("Constructed module path: {}", module_path);
            return Some(module_path);
        } else {
            debug!("No path parts found, using 'std' as module path");
            return Some("std".to_string());
        }
    }

    // Check for alloc crate patterns - handle "alloc/src/" pattern
    if let Some(pos) = filename.find(ALLOC_SRC_PREFIX) {
        debug!(
            "Found alloc library pattern in filename at position: {}",
            pos
        );
        let after_src = &filename[pos + ALLOC_SRC_PREFIX.len()..]; // Skip "alloc/src/"
        debug!("Path after 'alloc/src/': {}", after_src);

        let path_parts = process_path_parts(after_src);
        debug!("Filtered alloc path parts: {:?}", path_parts);

        if !path_parts.is_empty() {
            let module_path = format!("alloc::{}", path_parts.join("::"));
            debug!("Constructed alloc module path: {}", module_path);
            return Some(module_path);
        } else {
            debug!("No alloc path parts found, using 'alloc' as module path");
            return Some("alloc".to_string());
        }
    }

    // Check for core crate patterns - handle "core/src/" pattern
    if let Some(pos) = filename.find(CORE_SRC_PREFIX) {
        debug!(
            "Found core library pattern in filename at position: {}",
            pos
        );
        let after_src = &filename[pos + CORE_SRC_PREFIX.len()..]; // Skip "core/src/"
        debug!("Path after 'core/src/': {}", after_src);

        let path_parts = process_path_parts(after_src);
        debug!("Filtered core path parts: {:?}", path_parts);

        if !path_parts.is_empty() {
            let module_path = format!("core::{}", path_parts.join("::"));
            debug!("Constructed core module path: {}", module_path);
            return Some(module_path);
        } else {
            debug!("No core path parts found, using 'core' as module path");
            return Some("core".to_string());
        }
    }

    debug!(
        "No recognized library pattern found in filename: {}",
        filename
    );
    None
}

/// Parse struct kind and extract field information
///
/// This function analyzes the struct definition in rustdoc JSON to determine
/// the struct type (plain, tuple, or unit) and extracts field information
/// by looking up field IDs in the JSON index.
///
/// # JSON Structure Examples
///
/// ## Plain Struct (like `String`)
/// ```json
/// {
///   "kind": {
///     "plain": {
///       "fields": [5297]  // Array of field IDs
///     }
///   }
/// }
/// ```
///
/// ## Tuple Struct (like `struct Point(i32, i32)`)
/// ```json
/// {
///   "kind": {
///     "tuple": {
///       "fields": [1234, 1235]  // Array of field IDs for tuple elements
///     }
///   }
/// }
/// ```
///
/// ## Unit Struct (like `struct Unit;`)
/// ```json
/// {
///   "kind": {
///     "unit": {}
///   }
/// }
/// ```
///
/// # Field Resolution Process
///
/// 1. Extract field IDs from the `fields` array
/// 2. Look up each field ID in the main JSON index
/// 3. Parse field name, type, and visibility from the field JSON
/// 4. Build `FieldInfo` objects for each field
///
/// # Arguments
///
/// * `struct_info` - Mutable reference to the `StructInfo` being built
/// * `struct_obj` - The struct definition JSON object
/// * `full_json` - Complete rustdoc JSON for field lookups
///
/// # Returns
///
/// * `Ok(())` - Successfully parsed struct kind and fields
/// * `Err(QuarryError)` - Error occurred during field parsing
fn parse_struct_kind_and_fields(
    struct_info: &mut StructInfo,
    struct_obj: &serde_json::Map<String, Value>,
    full_json: &Value,
) -> Result<()> {
    debug!("Parsing struct kind for: {}", struct_info.name);

    // Check the struct kind in the rustdoc format: {"kind": {"plain": {"fields": [id1, id2, ...]}}}
    if let Some(kind) = struct_obj.get("kind") {
        if let Some(kind_obj) = kind.as_object() {
            if let Some(plain) = kind_obj.get("plain") {
                debug!("Found plain struct type for: {}", struct_info.name);
                if let Some(plain_obj) = plain.as_object() {
                    if let Some(field_ids) = plain_obj.get("fields").and_then(|f| f.as_array()) {
                        debug!(
                            "Found {} field IDs for struct: {}",
                            field_ids.len(),
                            struct_info.name
                        );
                        // Parse fields by looking up their IDs in the index
                        struct_info.fields =
                            parse_fields_by_ids(field_ids, full_json, &struct_info.simple_name)?;
                    }
                }
            } else if let Some(tuple) = kind_obj.get("tuple") {
                debug!("Found tuple struct type for: {}", struct_info.name);
                struct_info.is_tuple_struct = true;
                if let Some(tuple_obj) = tuple.as_object() {
                    if let Some(field_ids) = tuple_obj.get("fields").and_then(|f| f.as_array()) {
                        struct_info.fields =
                            parse_fields_by_ids(field_ids, full_json, &struct_info.simple_name)?;
                    }
                }
            } else if kind_obj.get("unit").is_some() {
                struct_info.is_unit_struct = true;
            }
        } else if kind.as_str() == Some("unit") {
            struct_info.is_unit_struct = true;
        }
    }

    Ok(())
}

/// Parse fields by looking up their IDs in the rustdoc JSON index
///
/// This function takes an array of field IDs and resolves them to complete
/// field information by looking up each ID in the main rustdoc JSON index.
/// It works for both plain structs (with named fields) and tuple structs
/// (with positional fields).
///
/// # JSON Structure Example
///
/// ## Field ID Array (from struct definition)
/// ```json
/// {
///   "fields": [5297, 6564, 6565]  // Array of field IDs to resolve
/// }
/// ```
///
/// ## Field JSON (looked up by ID in index)
/// For `String.vec` field (ID 5297):
/// ```json
/// {
///   "id": 5297,
///   "name": "vec",
///   "span": {
///     "filename": "alloc/src/string.rs",
///     "begin": [361, 5],
///     "end": [361, 17]
///   },
///   "visibility": {
///     "restricted": {
///       "parent": 5298,
///       "path": "::string"
///     }
///   },
///   "inner": {
///     "struct_field": {
///       "resolved_path": {
///         "path": "crate::vec::Vec",
///         "id": 241,
///         "args": {
///           "angle_bracketed": {
///             "args": [
///               {
///                 "type": {
///                   "primitive": "u8"
///                 }
///               }
///             ]
///           }
///         }
///       }
///     }
///   }
/// }
/// ```
///
/// ## Visibility Parsing
///
/// - `"public"` → field is public
/// - `{"restricted": {...}}` → field is private/restricted
/// - Missing → defaults to private
///
/// # Usage
///
/// - **Plain structs**: Fields have actual names like "vec", "len", etc.
/// - **Tuple structs**: Fields are numbered like "0", "1", "2", etc.
///
/// # Arguments
///
/// * `field_ids` - Array of field ID values from the struct definition
/// * `full_json` - Complete rustdoc JSON containing the index
/// * `struct_name` - Name of the parent struct (for field association)
///
/// # Returns
///
/// * `Ok(Vec<FieldInfo>)` - Successfully parsed field information
/// * `Err(QuarryError)` - Error during field lookup or parsing
fn parse_fields_by_ids(
    field_ids: &[Value],
    full_json: &Value,
    struct_name: &str,
) -> Result<Vec<FieldInfo>> {
    debug!(
        "Parsing {} field IDs for struct: {}",
        field_ids.len(),
        struct_name
    );
    let mut fields = Vec::new();

    if let Some(index) = full_json.get("index").and_then(|i| i.as_object()) {
        for (i, field_id) in field_ids.iter().enumerate() {
            if let Some(field_id_num) = field_id.as_u64() {
                let field_id_str = field_id_num.to_string();
                debug!(
                    "Looking up field {} (ID: {}) for struct {}",
                    i + 1,
                    field_id_str,
                    struct_name
                );

                if let Some(field_item) = index.get(&field_id_str).and_then(|f| f.as_object()) {
                    let field_name = field_item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let visibility = field_item
                        .get("visibility")
                        .and_then(|v| v.as_str())
                        .unwrap_or("private");

                    let is_public = visibility == "public";
                    debug!(
                        "Field '{}' visibility: {} (public: {})",
                        field_name, visibility, is_public
                    );

                    // Get field type from the struct_field inner data
                    let field_type = if let Some(field_inner) =
                        field_item.get("inner").and_then(|i| i.as_object())
                    {
                        if let Some(struct_field) = field_inner.get("struct_field") {
                            // The struct_field directly contains the type information
                            extract_type_name_from_json(struct_field)
                                .unwrap_or("unknown".to_string())
                        } else {
                            "unknown".to_string()
                        }
                    } else {
                        "unknown".to_string()
                    };

                    debug!(
                        "Parsed field: {} -> {} (public: {})",
                        field_name, field_type, is_public
                    );

                    fields.push(FieldInfo {
                        name: field_name,
                        type_name: field_type,
                        is_public,
                        struct_name: struct_name.to_string(),
                    });
                } else {
                    debug!("Could not find field item for ID: {}", field_id_str);
                }
            } else {
                debug!("Field ID is not a valid number: {:?}", field_id);
            }
        }
    } else {
        debug!("No index found in rustdoc JSON for field lookup");
    }

    debug!("Parsed {} fields for struct: {}", fields.len(), struct_name);
    Ok(fields)
}

/// Insert a struct with its full name as the key
///
/// Adds a struct to the cache using only its complete module path as the key.
/// This enforces the requirement for users to specify exact paths when querying.
///
/// # Arguments
///
/// * `types` - Mutable reference to the HashMap cache
/// * `struct_info` - The struct information to insert
fn insert_struct_with_full_name(types: &mut HashMap<String, StructInfo>, struct_info: StructInfo) {
    // Insert only with the full path - no variations
    debug!("Inserting struct with full name: {}", struct_info.name);
    types.insert(struct_info.name.clone(), struct_info);
}

/// Extract type name from rustdoc JSON type definition
///
/// This function parses the complex type structures in rustdoc JSON to extract
/// readable type names. It handles resolved paths, primitives, and generic types.
///
/// # JSON Type Examples
///
/// ## Resolved Path Type (e.g., Vec<u8>)
/// ```json
/// {
///   "resolved_path": {
///     "path": "crate::vec::Vec",
///     "id": 241,
///     "args": {
///       "angle_bracketed": {
///         "args": [
///           {
///             "type": {
///               "primitive": "u8"
///             }
///           }
///         ]
///       }
///     }
///   }
/// }
/// ```
/// Extracted as: "Vec<u8>"
///
/// ## Primitive Type (e.g., usize)
/// ```json
/// {
///   "primitive": "usize"
/// }
/// ```
/// Extracted as: "usize"
///
/// ## Generic Type (e.g., T)
/// ```json
/// {
///   "generic": "T"
/// }
/// ```
/// Extracted as: "T"
///
/// ## Tuple Type (e.g., (i32, String))
/// ```json
/// {
///   "tuple": [
///     {"primitive": "i32"},
///     {"resolved_path": {"path": "std::string::String"}}
///   ]
/// }
/// ```
/// Extracted as: "(i32, String)"
///
/// # Type Extraction Rules
///
/// 1. **resolved_path**: Extract last segment of path + format generic args
/// 2. **primitive**: Use primitive type name directly
/// 3. **generic**: Use generic parameter name
/// 4. **tuple**: Format as parenthesized comma-separated list
/// 5. **Unknown**: Return None for unhandled structures
///
/// # Arguments
///
/// * `type_value` - JSON value containing the type definition
///
/// # Returns
///
/// * `Some(String)` - Successfully extracted type name
/// * `None` - Unable to extract type (unhandled JSON structure)
fn extract_type_name_from_json(type_value: &Value) -> Option<String> {
    // Handle primitive types directly
    if let Some(primitive) = type_value.get("primitive").and_then(|p| p.as_str()) {
        return Some(primitive.to_string());
    }

    // Handle resolved_path types (like Vec<T>, RawVec<T, A>, etc.)
    if let Some(resolved_path) = type_value
        .get("resolved_path")
        .and_then(|rp| rp.as_object())
    {
        let path = resolved_path
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("UnknownPath");

        // Clean up the path - remove "crate::" prefix and convert to std:: if appropriate
        let clean_path = if path.starts_with(CRATE_PREFIX) {
            let without_crate = &path[CRATE_PREFIX.len()..];
            // Convert common crate paths to std equivalents
            match without_crate {
                "vec::Vec" => "Vec",
                "string::String" => "String",
                "collections::hash_map::HashMap" => "HashMap",
                "collections::hash_set::HashSet" => "HashSet",
                _ => without_crate,
            }
        } else {
            path
        };

        // Handle generic arguments
        if let Some(args) = resolved_path.get("args") {
            if let Some(angle_bracketed) = args.get("angle_bracketed").and_then(|ab| ab.as_object())
            {
                if let Some(args_array) = angle_bracketed.get("args").and_then(|a| a.as_array()) {
                    let type_args: Vec<String> = args_array
                        .iter()
                        .filter_map(|arg| {
                            if let Some(type_obj) = arg.get("type") {
                                extract_type_name_from_json(type_obj)
                            } else {
                                None
                            }
                        })
                        .collect();

                    if !type_args.is_empty() {
                        return Some(format!("{}<{}>", clean_path, type_args.join(", ")));
                    }
                }
            }
        }

        return Some(clean_path.to_string());
    }

    // Handle generic types
    if let Some(generic) = type_value.get("generic").and_then(|g| g.as_str()) {
        return Some(generic.to_string());
    }

    // No matching type pattern found
    None
}

/// Get struct information for a standard library type
///
/// This function retrieves detailed information about a Rust standard library struct,
/// including its fields and their types. It supports both exact module paths and 
/// common std:: aliases.
///
/// # Alias Support
///
/// The function automatically resolves common std:: aliases to their actual definitions:
/// - `std::string::String` → `alloc::string::String`  
/// - `std::vec::Vec` → `alloc::vec::Vec`
/// - `std::boxed::Box` → `alloc::boxed::Box`
/// - And other common std:: re-exports
///
/// # Examples
///
/// ```
/// use quarry::mine_stdlib_struct_info;
///
/// // ✅ Both of these work - std:: alias and exact path
/// let string_info1 = mine_stdlib_struct_info("std::string::String")?;
/// let string_info2 = mine_stdlib_struct_info("alloc::string::String")?;
/// // Both return the same information
///
/// let vec_info = mine_stdlib_struct_info("std::vec::Vec")?;
/// let hashmap_info = mine_stdlib_struct_info("std::collections::HashMap")?;
/// ```
///
/// # Arguments
///
/// * `name` - The full module path or std:: alias (e.g., "std::string::String")
///
/// # Returns
///
/// * `Ok(StructInfo)` - Detailed information about the struct including fields
/// * `Err(QuarryError::TypeNotFound)` - If the type name is not found
///
/// # Cache Behavior
///
/// The function uses a global cache that is initialized on first use. The cache
/// contains structs from the std, alloc, and core crates with their exact paths
/// as keys.
pub(crate) fn mine_stdlib_struct_info(name: &str) -> Result<StructInfo> {
    debug!("Mining stdlib struct info for: '{}'", name);

    // Get or initialize the cache
    let cache = STDLIB_CACHE.get_or_init(|| Mutex::new(None));
    let mut cache_guard = cache.lock().unwrap();

    // Initialize the cache if it's empty
    if cache_guard.is_none() {
        debug!("Cache not initialized, initializing stdlib types cache");
        match init_stdlib_types() {
            Ok(types) => {
                debug!("Successfully initialized cache with {} types", types.len());
                *cache_guard = Some(types);
            }
            Err(e) => {
                debug!("Failed to initialize stdlib types cache: {:?}", e);
                return Err(e);
            }
        }
    } else {
        debug!("Using existing initialized cache");
    }

    let stdlib_types = cache_guard.as_ref().unwrap();

    // Try exact match first
    debug!("Looking for exact match for: '{}'", name);
    if let Some(info) = stdlib_types.get(name) {
        debug!("Found exact match for: '{}'", name);
        return Ok(info.clone());
    }

    // Try alias resolution
    debug!("No exact match found, trying alias resolution for: '{}'", name);
    if let Some(actual_path) = resolve_std_alias(name) {
        debug!("Resolved '{}' to actual path: '{}'", name, actual_path);
        if let Some(info) = stdlib_types.get(&actual_path) {
            debug!("Found struct via alias resolution: '{}'", name);
            
            // Create a new StructInfo with the alias name (what the user requested)
            // instead of the internal path name
            let mut aliased_info = info.clone();
            aliased_info.name = name.to_string();
            
            // Update the module path to match the alias
            if let Some(pos) = name.rfind("::") {
                aliased_info.module_path = name[..pos].to_string();
            }
            
            // Update the simple name (should be the same, but just to be consistent)
            if let Some(pos) = name.rfind("::") {
                aliased_info.simple_name = name[pos + 2..].to_string();
            }
            
            debug!("Created aliased StructInfo: '{}' -> module: '{}', simple: '{}'", 
                   aliased_info.name, aliased_info.module_path, aliased_info.simple_name);
            
            return Ok(aliased_info);
        } else {
            debug!("Alias resolved but actual type not found: '{}'", actual_path);
        }
    }

    debug!(
        "No match found for '{}' (tried exact match and alias resolution)",
        name
    );
    Err(QuarryError::TypeNotFound(format!(
        "Type '{}' not found. Please provide the full module path (e.g., 'std::string::String', 'alloc::string::String')",
        name
    )))
}

/// Resolve std:: aliases to their actual module paths
///
/// This function provides comprehensive std:: alias resolution based on the official
/// Rust documentation from https://doc.rust-lang.org/nightly/std/index.html
///
/// # Examples
///
/// - `std::string::String` → `alloc::string::String`
/// - `std::vec::Vec` → `alloc::vec::Vec`
/// - `std::boxed::Box` → `alloc::boxed::Box`
///
/// # Arguments
///
/// * `name` - The std:: path to resolve
///
/// # Returns
///
/// * `Some(String)` - The actual module path if an alias is found
/// * `None` - If no alias mapping exists for the given path
fn resolve_std_alias(name: &str) -> Option<String> {
    debug!("Resolving std alias for: '{}'", name);

    let alias = match name {
        // Module alloc (see https://doc.rust-lang.org/nightly/std/alloc/index.html)
        "std::alloc::Layout" => Some("core::alloc::layout::Layout"),
        "std::alloc::LayoutError" => Some("core::alloc::layout::LayoutError"),
        "std::alloc::System" => Some("std::alloc::System"), // Not aliased

        // Module any (see https://doc.rust-lang.org/nightly/std/any/index.html)
        "std::any::TypeId" => Some("core::any::TypeId"),

        // Module array (see https://doc.rust-lang.org/nightly/std/array/index.html)
        "std::array::IntoIter" => Some("core::array::iter::IntoIter"),
        "std::array::TryFromSliceError" => Some("core::array::TryFromSliceError"),

        // Module ascii (see https://doc.rust-lang.org/nightly/std/ascii/index.html)
        "std::ascii::EscapeDefault" => Some("core::ascii::EscapeDefault"),

        // Module backtrace (see https://doc.rust-lang.org/nightly/std/backtrace/index.html)
        "std::backtrace::Backtrace" => Some("std::backtrace::Backtrace"), // Not aliased

        // Module boxed (see https://doc.rust-lang.org/nightly/std/boxed/index.html)
        "std::boxed::Box" => Some("alloc::boxed::Box"),

        // Module cell (https://doc.rust-lang.org/nightly/std/cell/index.html)
        "std::cell::BorrowError" => Some("core::cell::BorrowError"),
        "std::cell::BorrowMutError" => Some("core::cell::BorrowMutError"),
        "std::cell::Cell" => Some("core::cell::Cell"),
        "std::cell::LazyCell" => Some("core::cell::lazy::LazyCell"),
        "std::cell::OnceCell" => Some("core::cell::once::OnceCell"),
        "std::cell::Ref" => Some("core::cell::Ref"),
        "std::cell::RefCell" => Some("core::cell::RefCell"),
        "std::cell::RefMut" => Some("core::cell::RefMut"),
        "std::cell::UnsafeCell" => Some("core::cell::UnsafeCell"),

        // Module char (see https://doc.rust-lang.org/nightly/std/char/index.html)
        "std::char::CharTryFromError" => Some("core::char::convert::CharTryFromError"),
        "std::char::DecodeUtf16" => Some("core::char::decode::DecodeUtf16"),
        "std::char::DecodeUtf16Error" => Some("core::char::decode::DecodeUtf16Error"),
        "std::char::EscapeDebug" => Some("core::char::EscapeDebug"),
        "std::char::EscapeDefault" => Some("core::char::EscapeDefault"),
        "std::char::EscapeUnicode" => Some("core::char::EscapeUnicode"),
        "std::char::ParseCharError" => Some("core::char::convert::ParseCharError"),
        "std::char::ToLowercase" => Some("core::char::ToLowercase"),
        "std::char::ToUppercase" => Some("core::char::ToUppercase"),
        "std::char::TryFromCharError" => Some("core::char::TryFromCharError"),

        // Module cmp (see https://doc.rust-lang.org/nightly/std/cmp/index.html)
        "std::cmp::Reverse" => Some("core::cmp::Reverse"),

        // Module collections (see https://doc.rust-lang.org/nightly/std/collections/index.html)
        "std::collections::BTreeMap" => Some("alloc::collections::btree::map::BTreeMap"),
        "std::collections::BTreeSet" => Some("alloc::collections::btree::set::BTreeSet"),
        "std::collections::BinaryHeap" => Some("alloc::collections::binary_heap::BinaryHeap"),
        "std::collections::HashMap" => Some("std::collections::hash::map::HashMap"),
        "std::collections::HashSet" => Some("std::collections::hash::set::HashSet"),
        "std::collections::LinkedList" => Some("alloc::collections::linked_list::LinkedList"),
        "std::collections::TryReserveError" => Some("alloc::collections::TryReserveError"),
        "std::collections::VecDeque" => Some("alloc::collections::vec_deque::VecDeque"),

        // Module ffi (see https://doc.rust-lang.org/nightly/std/ffi/index.html)
        "std::ffi::CStr" => Some("core::ffi::c_str::CStr"),
        "std::ffi::CString" => Some("alloc::ffi::c_str::CString"),
        "std::ffi::FromBytesUntilNulError" => Some("core::ffi::c_str::FromBytesUntilNulError"),
        "std::ffi::FromVecWithNulError" => Some("alloc::ffi::c_str::FromVecWithNulError"),
        "std::ffi::IntoStringError" => Some("alloc::ffi::c_str::IntoStringError"),
        "std::ffi::NulError" => Some("alloc::ffi::c_str::NulError"),
        "std::ffi::OsStr" => Some("std::ffi::os_str::OsStr"),
        "std::ffi::OsString" => Some("std::ffi::os_str::OsString"),

        // Module fmt (see https://doc.rust-lang.org/nightly/std/fmt/index.html)
        "std::fmt::Arguments" => Some("core::fmt::Arguments"),
        "std::fmt::DebugList" => Some("core::fmt::builder::DebugList"),
        "std::fmt::DebugMap" => Some("core::fmt::builder::DebugMap"),
        "std::fmt::DebugSet" => Some("core::fmt::builder::DebugSet"),
        "std::fmt::DebugStruct" => Some("core::fmt::builder::DebugStruct"),
        "std::fmt::DebugTuple" => Some("core::fmt::builder::DebugTuple"),
        "std::fmt::Error" => Some("core::fmt::Error"),
        "std::fmt::Formatter" => Some("core::fmt::Formatter"),

        // Module fs (see https://doc.rust-lang.org/nightly/std/fs/index.html)
        "std::fs::DirBuilder" => Some("std::fs::DirBuilder"), // Not aliased
        "std::fs::DirEntry" => Some("std::fs::DirEntry"), // Not aliased
        "std::fs::File" => Some("std::fs::File"), // Not aliased
        "std::fs::FileTimes" => Some("std::fs::FileTimes"), // Not aliased
        "std::fs::FileType" => Some("std::fs::FileType"), // Not aliased
        "std::fs::Metadata" => Some("std::fs::Metadata"), // Not aliased
        "std::fs::OpenOptions" => Some("std::fs::OpenOptions"), // Not aliased
        "std::fs::Permissions" => Some("std::fs::Permissions"), // Not aliased
        "std::fs::ReadDir" => Some("std::fs::ReadDir"), // Not aliased

        // Module future (see https://doc.rust-lang.org/nightly/std/future/index.html)
        "std::future::Pending" => Some("core::future::pending::Pending"),
        "std::future::PollFn" => Some("core::future::poll_fn::PollFn"),
        "std::future::Ready" => Some("core::future::ready::Ready"),

        // Module hash (see https://doc.rust-lang.org/nightly/std/hash/index.html)
        "std::hash::BuildHasherDefault" => Some("core::hash::BuildHasherDefault"),
        "std::hash::DefaultHasher" => Some("std::hash::random::DefaultHasher"),
        "std::hash::RandomState" => Some("std::hash::random::RandomState"),

        // Module io (see https://doc.rust-lang.org/nightly/std/io/index.html)
        "std::io::BufReader" => Some("std::io::buffered::bufreader::BufReader"),
        "std::io::BufWriter" => Some("std::io::buffered::bufwriter::BufWriter"),
        "std::io::Bytes" => Some("std::io::Bytes"), // Not aliased
        "std::io::Chain" => Some("std::io::Chain"), // Not aliased
        "std::io::Cursor" => Some("std::io::cursor::Cursor"),
        "std::io::Empty" => Some("std::io::util::Empty"),
        "std::io::Error" => Some("std::io::error::Error"),
        "std::io::IntoInnerError" => Some("std::io::buffered::IntoInnerError"),
        "std::io::IoSlice" => Some("std::io::IoSlice"), // Not aliased
        "std::io::IoSliceMut" => Some("std::io::IoSliceMut"), // Not aliased
        "std::io::LineWriter" => Some("std::io::buffered::linewriter::LineWriter"),
        "std::io::Lines" => Some("std::io::Lines"), // Not aliased
        "std::io::PipeReader" => Some("std::io::pipe::PipeReader"),
        "std::io::PipeWriter" => Some("std::io::pipe::PipeWriter"),
        "std::io::Repeat" => Some("std::io::util::Repeat"),
        "std::io::Sink" => Some("std::io::util::Sink"),
        "std::io::Split" => Some("std::io::Split"), // Not aliased
        "std::io::Stderr" => Some("std::io::stdio::Stderr"),
        "std::io::StderrLock" => Some("std::io::stdio::StderrLock"),
        "std::io::Stdin" => Some("std::io::stdio::Stdin"),
        "std::io::StdinLock" => Some("std::io::stdio::StdinLock"),
        "std::io::Stdout" => Some("std::io::stdio::Stdout"),
        "std::io::StdoutLock" => Some("std::io::StdoutLock"),
        "std::io::Take" => Some("std::io::Take"), // Not aliased
        "std::io::WriterPanicked" => Some("std::io::buffered::bufwriter::WriterPanicked"),

        // Module iter (see https://doc.rust-lang.org/nightly/std/iter/index.html)
        "std::iter::Chain" => Some("core::iter::adapters::chain::Chain"),
        "std::iter::Cloned" => Some("core::iter::adapters::cloned::Cloned"),
        "std::iter::Copied" => Some("core::iter::adapters::copied::Copied"),
        "std::iter::Cycle" => Some("core::iter::adapters::cycle::Cycle"),
        "std::iter::Empty" => Some("core::iter::sources::empty::Empty"),
        "std::iter::Enumerate" => Some("core::iter::adapters::enumerate::Enumerate"),
        "std::iter::Filter" => Some("core::iter::adapters::filter::Filter"),
        "std::iter::FilterMap" => Some("core::iter::adapters::filter_map::FilterMap"),
        "std::iter::FlatMap" => Some("core::iter::adapters::flatten::FlatMap"),
        "std::iter::Flatten" => Some("core::iter::adapters::flatten::Flatten"),
        "std::iter::FromFn" => Some("core::iter::sources::from_fn::FromFn"),
        "std::iter::Fuse" => Some("core::iter::adapters::fuse::Fuse"),
        "std::iter::Inspect" => Some("core::iter::adapters::inspect::Inspect"),
        "std::iter::Map" => Some("core::iter::adapters::map::Map"),
        "std::iter::MapWhile" => Some("core::iter::adapters::map_while::MapWhile"),
        "std::iter::Once" => Some("core::iter::sources::once::Once"),
        "std::iter::OnceWith" => Some("core::iter::sources::once_with::OnceWith"),
        "std::iter::Peekable" => Some("core::iter::adapters::peekable::Peekable"),
        "std::iter::Repeat" => Some("core::iter::sources::repeat::Repeat"),
        "std::iter::RepeatN" => Some("core::iter::sources::repeat_n::RepeatN"),
        "std::iter::RepeatWith" => Some("core::iter::sources::repeat_with::RepeatWith"),
        "std::iter::Rev" => Some("core::iter::adapters::rev::Rev"),
        "std::iter::Scan" => Some("core::iter::adapters::scan::Scan"),
        "std::iter::Skip" => Some("core::iter::adapters::skip::Skip"),
        "std::iter::SkipWhile" => Some("core::iter::adapters::skip_while::SkipWhile"),
        "std::iter::StepBy" => Some("core::iter::adapters::step_by::StepBy"),
        "std::iter::Successors" => Some("core::iter::sources::successors::Successors"),
        "std::iter::Take" => Some("core::iter::adapters::take::Take"),
        "std::iter::TakeWhile" => Some("core::iter::adapters::take_while::TakeWhile"),
        "std::iter::Zip" => Some("core::iter::adapters::zip::Zip"),

        // Module marker (see https://doc.rust-lang.org/nightly/std/marker/index.html)
        "std::marker::PhantomData" => Some("core::marker::PhantomData"),
        "std::marker::PhantomPinned" => Some("core::marker::PhantomPinned"),

        // Module mem (see https://doc.rust-lang.org/nightly/std/mem/index.html)
        "std::mem::Discriminant" => Some("core::mem::Discriminant"),
        "std::mem::ManuallyDrop" => Some("core::mem::manually_drop::ManuallyDrop"),

        // Module net (see https://doc.rust-lang.org/nightly/std/net/index.html)
        "std::net::AddrParseError" => Some("core::net::parser::AddrParseError"),
        "std::net::Incoming" => Some("std::net::tcp::Incoming"),
        "std::net::Ipv4Addr" => Some("core::net::ip_addr::Ipv4Addr"),
        "std::net::Ipv6Addr" => Some("core::net::ip_addr::Ipv6Addr"),
        "std::net::SocketAddrV4" => Some("core::net::socket_addr::SocketAddrV4"),
        "std::net::SocketAddrV6" => Some("core::net::socket_addr::SocketAddrV6"),
        "std::net::TcpListener" => Some("std::net::tcp::TcpListener"),
        "std::net::TcpStream" => Some("std::net::tcp::TcpStream"),
        "std::net::UdpSocket" => Some("std::net::udp::UdpSocket"),

        // Module num (see https://doc.rust-lang.org/nightly/std/num/index.html)
        "std::num::NonZero" => Some("core::num::nonzero::NonZero"),
        "std::num::ParseFloatError" => Some("core::num::dec2flt::ParseFloatError"),
        "std::num::ParseIntError" => Some("core::num::error::ParseIntError"),
        "std::num::Saturating" => Some("core::num::saturating::Saturating"),
        "std::num::TryFromIntError" => Some("core::num::error::TryFromIntError"),
        "std::num::Wrapping" => Some("core::num::wrapping::Wrapping"),

        // Module ops (see https://doc.rust-lang.org/nightly/std/ops/index.html)
        "std::ops::Range" => Some("core::ops::range::Range"),
        "std::ops::RangeFrom" => Some("core::ops::range::RangeFrom"),
        "std::ops::RangeFull" => Some("core::ops::range::RangeFull"),
        "std::ops::RangeInclusive" => Some("core::ops::range::RangeInclusive"),
        "std::ops::RangeTo" => Some("core::ops::range::RangeTo"),
        "std::ops::RangeToInclusive" => Some("core::ops::range::RangeToInclusive"),

        // Module option (see https://doc.rust-lang.org/nightly/std/option/index.html)
        "std::option::IntoIter" => Some("core::option::IntoIter"),
        "std::option::Iter" => Some("core::option::Iter"),
        "std::option::IterMut" => Some("core::option::IterMut"),

        // Module fd (see https://doc.rust-lang.org/nightly/std/os/fd/index.html)
        "std::os::fd::BorrowedFd" => Some("std::os::fd::owned::BorrowedFd"),
        "std::os::fd::OwnedFd" => Some("std::os::fd::owned::OwnedFd"),

        // Module panic (see https://doc.rust-lang.org/nightly/std/panic/index.html)
        "std::panic::AssertUnwindSafe" => Some("core::panic::unwind_safe::AssertUnwindSafe"),
        "std::panic::Location" => Some("core::panic::location::Location"),
        "std::panic::PanicHookInfo" => Some("std::panic::PanicHookInfo"), // Not aliased

        // Module path (see https://doc.rust-lang.org/nightly/std/path/index.html)
        "std::path::Ancestors" => Some("std::path::Ancestors"), // Not aliased
        "std::path::Components" => Some("std::path::Components"), // Not aliased
        "std::path::Display" => Some("std::path::Display"), // Not aliased
        "std::path::Iter" => Some("std::path::Iter"), // Not aliased
        "std::path::Path" => Some("std::path::Path"), // Not aliased
        "std::path::PathBuf" => Some("std::path::PathBuf"), // Not aliased
        "std::path::PrefixComponent" => Some("std::path::PrefixComponent"), // Not aliased
        "std::path::StripPrefixError" => Some("std::path::StripPrefixError"), // Not aliased

        // Module pin (see https://doc.rust-lang.org/nightly/std/pin/index.html)
        "std::pin::Pin" => Some("core::pin::Pin"),

        // Module process (see https://doc.rust-lang.org/nightly/std/process/index.html)
        "std::process::Child" => Some("std::process::Child"), // Not aliased
        "std::process::ChildStderr" => Some("std::process::ChildStderr"), // Not aliased
        "std::process::ChildStdin" => Some("std::process::ChildStdin"), // Not aliased
        "std::process::ChildStdout" => Some("std::process::ChildStdout"), // Not aliased
        "std::process::Command" => Some("std::process::Command"), // Not aliased
        "std::process::CommandArgs" => Some("std::process::CommandArgs"), // Not aliased
        "std::process::CommandEnvs" => Some("std::process::CommandEnvs"), // Not aliased
        "std::process::ExitCode" => Some("std::process::ExitCode"), // Not aliased
        "std::process::ExitStatus" => Some("std::process::ExitStatus"), // Not aliased
        "std::process::Output" => Some("std::process::Output"), // Not aliased
        "std::process::Stdio" => Some("std::process::Stdio"), // Not aliased

        // Module ptr (see https://doc.rust-lang.org/nightly/std/ptr/index.html)
        "std::ptr::NonNull" => Some("core::ptr::non_null::NonNull"),

        // Module rc (see https://doc.rust-lang.org/nightly/std/rc/index.html)
        "std::rc::Rc" => Some("alloc::rc::Rc"),
        "std::rc::Weak" => Some("alloc::rc::Weak"),

        // Module result (see https://doc.rust-lang.org/nightly/std/result/index.html)
        "std::result::IntoIter" => Some("core::result::IntoIter"),
        "std::result::Iter" => Some("core::result::Iter"),
        "std::result::IterMut" => Some("core::result::IterMut"),

        // Module slice (see https://doc.rust-lang.org/nightly/std/slice/index.html)
        "std::slice::ChunkBy" => Some("core::slice::iter::ChunkBy"),
        "std::slice::ChunkByMut" => Some("core::slice::iter::ChunkByMut"),
        "std::slice::Chunks" => Some("core::slice::iter::Chunks"),
        "std::slice::ChunksExact" => Some("core::slice::iter::ChunksExact"),
        "std::slice::ChunksExactMut" => Some("core::slice::iter::ChunksExactMut"),
        "std::slice::ChunksMut" => Some("core::slice::iter::ChunksMut"),
        "std::slice::EscapeAscii" => Some("core::slice::ascii::EscapeAscii"),
        "std::slice::Iter" => Some("core::slice::iter::Iter"),
        "std::slice::IterMut" => Some("core::slice::iter::IterMut"),
        "std::slice::RChunks" => Some("core::slice::iter::RChunks"),
        "std::slice::RChunksExact" => Some("core::slice::iter::RChunksExact"),
        "std::slice::RChunksExactMut" => Some("core::slice::iter::RChunksExactMut"),
        "std::slice::RChunksMut" => Some("core::slice::iter::RChunksMut"),
        "std::slice::RSplit" => Some("core::slice::iter::RSplit"),
        "std::slice::RSplitMut" => Some("core::slice::iter::RSplitMut"),
        "std::slice::RSplitN" => Some("core::slice::iter::RSplitN"),
        "std::slice::RSplitNMut" => Some("core::slice::iter::RSplitNMut"),
        "std::slice::Split" => Some("core::slice::iter::Split"),
        "std::slice::SplitInclusive" => Some("core::slice::iter::SplitInclusive"),
        "std::slice::SplitInclusiveMut" => Some("core::slice::iter::SplitInclusiveMut"),
        "std::slice::SplitMut" => Some("core::slice::iter::SplitMut"),
        "std::slice::SplitN" => Some("core::slice::iter::SplitN"),
        "std::slice::SplitNMut" => Some("core::slice::iter::SplitNMut"),
        "std::slice::Windows" => Some("core::slice::iter::Windows"),

        // Module str (see https://doc.rust-lang.org/nightly/std/str/index.html)
        "std::str::Bytes" => Some("core::str::iter::Bytes"),
        "std::str::CharIndices" => Some("core::str::iter::CharIndices"),
        "std::str::Chars" => Some("core::str::iter::Chars"),
        "std::str::EncodeUtf16" => Some("core::str::iter::EncodeUtf16"),
        "std::str::EscapeDebug" => Some("core::str::iter::EscapeDebug"),
        "std::str::EscapeDefault" => Some("core::str::iter::EscapeDefault"),
        "std::str::EscapeUnicode" => Some("core::str::iter::EscapeUnicode"),
        "std::str::Lines" => Some("core::str::iter::Lines"),
        "std::str::MatchIndices" => Some("core::str::iter::MatchIndices"),
        "std::str::Matches" => Some("core::str::iter::Matches"),
        "std::str::ParseBoolError" => Some("core::str::error::ParseBoolError"),
        "std::str::RMatchesIndices" => Some("core::str::iter::RMatchesIndices"),
        "std::str::RMatches" => Some("core::str::iter::RMatches"),
        "std::str::RSplit" => Some("core::str::iter::RSplit"),
        "std::str::RSplitN" => Some("core::str::iter::RSplitN"),
        "std::str::RSplitTerminator" => Some("core::str::iter::RSplitTerminator"),
        "std::str::Split" => Some("core::str::iter::Split"),
        "std::str::SplitAsciiWhitespace" => Some("core::str::iter::SplitAsciiWhitespace"),
        "std::str::SplitInclusive" => Some("core::str::iter::SplitInclusive"),
        "std::str::SplitN" => Some("core::str::iter::SplitN"),
        "std::str::SplitTerminator" => Some("core::str::iter::SplitTerminator"),
        "std::str::SplitWhitespace" => Some("core::str::iter::SplitWhitespace"),
        "std::str::Utf8Chunk" => Some("core::str::lossy::Utf8Chunk"),
        "std::str::Utf8Chunks" => Some("core::str::lossy::Utf8Chunks"),
        "std::str::Utf8Error" => Some("core::str::error::Utf8Error"),

        // Module string (see https://doc.rust-lang.org/nightly/std/string/index.html)
        "std::string::Drain" => Some("alloc::string::Drain"),
        "std::string::FromUtf8Error" => Some("alloc::string::FromUtf8Error"),
        "std::string::FromUtf16Error" => Some("alloc::string::FromUtf16Error"),
        "std::string::String" => Some("alloc::string::String"),

        // Module sync (see https://doc.rust-lang.org/nightly/std/sync/index.html)
        "std::sync::Arc" => Some("alloc::sync::Arc"),
        "std::sync::Barrier" => Some("std::sync::Barrier"), // Not aliased
        "std::sync::BarrierWaitResult" => Some("std::sync::BarrierWaitResult"), // Not aliased
        "std::sync::Condvar" => Some("std::sync::poison::condvar::Condvar"),
        "std::sync::LazyLock" => Some("std::sync::lazy_lock::LazyLock"),
        "std::sync::Mutex" => Some("std::sync::poison::mutex::Mutex"),
        "std::sync::MutexGuard" => Some("std::sync::poison::mutex::MutexGuard"),
        "std::sync::Once" => Some("std::sync::poison::once::Once"),
        "std::sync::OnceLock" => Some("std::sync::once_lock::OnceLock"),
        "std::sync::OnceState" => Some("std::sync::poison::once::OnceState"),
        "std::sync::PoisonError" => Some("std::sync::poison::PoisonError"),
        "std::sync::RwLock" => Some("std::sync::poison::rwlock::RwLock"),
        "std::sync::RwLockReadGuard" => Some("std::sync::poison::rwlock::RwLockReadGuard"),
        "std::sync::RwLockWriteGuard" => Some("std::sync::poison::rwlock::RwLockWriteGuard"),
        "std::sync::WaitTimeoutResult" => Some("std::sync::poison::condvar::WaitTimeoutResult"),
        "std::sync::Weak" => Some("alloc::sync::Weak"),

        // Module task (see https://doc.rust-lang.org/nightly/std/task/index.html)
        "std::task::RawWakerVTable" => Some("core::task::wake::RawWakerVTable"),
        "std::task::Waker" => Some("core::task::wake::Waker"),
        "std::task::Context" => Some("core::task::wake::Context"),
        "std::task::RawWaker" => Some("core::task::wake::RawWaker"),

        // Module thread (see https://doc.rust-lang.org/nightly/std/thread/index.html)
        "std::thread::AccessError" => Some("std::thread::local::AccessError"),
        "std::thread::Builder" => Some("std::thread::Builder"), // Not aliased
        "std::thread::JoinHandle" => Some("std::thread::JoinHandle"), // Not aliased
        "std::thread::LocalKey" => Some("std::thread::local::LocalKey"),
        "std::thread::Scope" => Some("std::thread::scoped::Scope"),
        "std::thread::ScopedJoinHandle" => Some("std::thread::scoped::ScopedJoinHandle"),
        "std::thread::Thread" => Some("std::thread::Thread"), // Not aliased
        "std::thread::ThreadId" => Some("std::thread::ThreadId"), // Not aliased

        // Module time (see https://doc.rust-lang.org/nightly/std/time/index.html)
        "std::time::Duration" => Some("core::time::Duration"),
        "std::time::Instant" => Some("std::time::Instant"), // Not aliased
        "std::time::SystemTime" => Some("std::time::SystemTime"), // Not aliased
        "std::time::SystemTimeError" => Some("std::time::SystemTimeError"), // Not aliased
        "std::time::TryFromFloatSecsError" => Some("core::time::TryFromFloatSecsError"),

        // Module vec (see https://doc.rust-lang.org/nightly/std/vec/index.html)
        "std::vec::Drain" => Some("alloc::vec::Drain"),
        "std::vec::ExtractIf" => Some("alloc::vec::ExtractIf"),
        "std::vec::IntoIter" => Some("alloc::vec::IntoIter"),
        "std::vec::Splice" => Some("alloc::vec::Splice"),
        "std::vec::Vec" => Some("alloc::vec::Vec"),

        _ => None,
    };
    
    if let Some(resolved) = alias {
        debug!("Resolved '{}' to '{}'", name, resolved);
        Some(resolved.to_string())
    } else {
        debug!("No alias found for '{}'", name);
        None
    }
}

/// Get a list of all available standard library struct types
///
/// Returns a sorted list of all struct types found in the std, alloc, and core crates.
/// All names are returned with their full module paths for use with `mine_stdlib_struct_info`.
///
/// # Examples
///
/// ```
/// use quarry::list_stdlib_structs;
///
/// let structs = list_stdlib_structs()?;
/// for struct_name in structs {
///     println!("{}", struct_name);
///     // Example output:
///     // alloc::string::String
///     // alloc::vec::Vec
///     // std::collections::HashMap
///     // core::option::Option
/// }
/// ```
///
/// # Returns
///
/// * `Ok(Vec<String>)` - Sorted list of all available struct types with full paths
/// * `Err(QuarryError)` - If the standard library cache cannot be initialized
///
/// # Performance
///
/// This function may take some time on first call as it initializes the cache by
/// parsing rustdoc JSON from the standard library. Subsequent calls are fast.
pub(crate) fn list_stdlib_structs() -> Result<Vec<String>> {
    debug!("Listing all stdlib structs");

    let cache = STDLIB_CACHE.get_or_init(|| Mutex::new(None));
    let mut cache_guard = cache.lock().unwrap();

    // Initialize the cache if it's empty
    if cache_guard.is_none() {
        debug!("Cache not initialized, initializing for struct listing");
        match init_stdlib_types() {
            Ok(types) => {
                debug!("Initialized cache with {} types for listing", types.len());
                *cache_guard = Some(types);
            }
            Err(e) => {
                debug!("Failed to initialize cache for listing: {:?}", e);
                return Err(e);
            }
        }
    }

    let stdlib_types = cache_guard.as_ref().unwrap();
    let mut names: Vec<String> = stdlib_types.keys().cloned().collect();
    names.sort();

    debug!("Found {} stdlib struct names", names.len());
    Ok(names)
}

/// Check if a type name refers to a standard library struct
///
/// Returns true if the given type name (with full module path) exists in the
/// standard library cache. Requires exact module paths.
///
/// # Examples
///
/// ```
/// use quarry::is_stdlib_struct;
///
/// // ✅ These will return true (if std lib is available)
/// assert!(is_stdlib_struct("alloc::string::String"));
/// assert!(is_stdlib_struct("alloc::vec::Vec"));
/// assert!(is_stdlib_struct("std::collections::HashMap"));
///
/// // ❌ These will return false - requires full paths
/// assert!(!is_stdlib_struct("String"));
/// assert!(!is_stdlib_struct("Vec"));
/// assert!(!is_stdlib_struct("NonExistentStruct"));
/// ```
///
/// # Arguments
///
/// * `name` - The full module path of the struct to check
///
/// # Returns
///
/// * `true` - If the struct exists in the standard library cache
/// * `false` - If the struct is not found or cache initialization fails
pub(crate) fn is_stdlib_struct(name: &str) -> bool {
    debug!("Checking if '{}' is a stdlib struct", name);
    let result = mine_stdlib_struct_info(name).is_ok();
    debug!("Result for '{}': {}", name, result);
    result
}

/// Clear the stdlib cache (useful for testing or if you want to refresh)
pub(crate) fn clear_cache() {
    debug!("Clearing stdlib cache");
    if let Some(cache) = STDLIB_CACHE.get() {
        let mut cache_guard = cache.lock().unwrap();
        *cache_guard = None;
        debug!("Stdlib cache cleared successfully");
    } else {
        debug!("Stdlib cache was not initialized, nothing to clear");
    }
}

/// Get cache statistics
pub(crate) fn cache_stats() -> Result<(usize, bool)> {
    debug!("Getting cache statistics");
    let cache = STDLIB_CACHE.get_or_init(|| Mutex::new(None));
    let cache_guard = cache.lock().unwrap();

    let stats = match cache_guard.as_ref() {
        Some(types) => {
            debug!("Cache is initialized with {} types", types.len());
            (types.len(), true)
        }
        None => {
            debug!("Cache is not initialized");
            (0, false)
        }
    };

    Ok(stats)
}
