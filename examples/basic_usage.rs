//! Basic Usage Example for Quarry
//!
//! This example demonstrates the fundamental features of Quarry for analyzing
//! Rust standard library types. It shows how to:
//! - Query specific struct information
//! - Access field details including private fields
//! - Work with different crate modules (std, alloc, core)

use quarry::{QuarryError, mine_struct_info};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to see debug output (optional)
    // Run with: RUST_LOG=quarry=debug cargo run --example basic_usage
    env_logger::init();

    println!("🔍 Quarry - Basic Usage Example");
    println!("=====================================\n");

    // Example 1: Analyze String from alloc crate
    println!("📝 Example 1: Analyzing alloc::string::String");
    analyze_struct("alloc::string::String")?;

    println!("\n{}\n", "─".repeat(50));

    // Example 2: Analyze Vec from alloc crate
    println!("📋 Example 2: Analyzing alloc::vec::Vec");
    analyze_struct("alloc::vec::Vec")?;

    println!("\n{}\n", "─".repeat(50));

    // Example 3: Analyze HashMap from std collections
    println!("🗺️  Example 3: Analyzing std::collections::HashMap");
    analyze_struct("std::collections::HashMap")?;

    println!("\n{}\n", "─".repeat(50));

    // Example 4: Analyze ManuallyDrop from std crate
    println!("🎯 Example 4: Analyzing std::mem::ManuallyDrop");
    analyze_struct("std::mem::ManuallyDrop")?;

    println!("\n{}\n", "─".repeat(50));

    // Example 5: Demonstrate error handling
    println!("❌ Example 5: Error Handling");
    demonstrate_error_handling();

    println!("\n✅ Basic usage examples completed!");
    Ok(())
}

/// Analyzes a single struct and displays detailed information
fn analyze_struct(struct_name: &str) -> Result<(), QuarryError> {
    println!("Analyzing: {}", struct_name);

    match mine_struct_info(struct_name) {
        Ok(info) => {
            // Basic information
            println!("  ✓ Found struct successfully!");
            println!("  📛 Full name: {}", info.name);
            println!("  🏷️  Simple name: {}", info.simple_name);
            println!("  📂 Module path: {}", info.module_path);

            // Struct characteristics
            println!("  🔧 Struct type:");
            if info.is_unit_struct {
                println!("    • Unit struct (no fields)");
            } else if info.is_tuple_struct {
                println!("    • Tuple struct (positional fields)");
            } else {
                println!("    • Named struct (named fields)");
            }

            // Field information
            println!("  📊 Fields: {} total", info.fields.len());
            if !info.fields.is_empty() {
                println!("    Field details:");
                for (i, field) in info.fields.iter().enumerate() {
                    let visibility = if field.is_public {
                        "🌐 public"
                    } else {
                        "🔒 private"
                    };
                    println!(
                        "    {}. {} : {} ({})",
                        i + 1,
                        field.name,
                        field.type_name,
                        visibility
                    );
                }
            } else {
                println!(
                    "    No fields accessible (may be opaque or have complex internal structure)"
                );
            }
        }
        Err(e) => {
            println!("  ❌ Error: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Demonstrates error handling for invalid struct names
fn demonstrate_error_handling() {
    println!("Trying to analyze invalid struct names...\n");

    let invalid_names = vec![
        "String",                   // Missing full path
        "Vec",                      // Missing full path
        "core::option::Option",     // This is an enum, not a struct
        "NonExistent",              // Doesn't exist
        "my::custom::Type",         // Not a stdlib type
    ];

    for name in invalid_names {
        println!("  Trying: {}", name);
        match mine_struct_info(name) {
            Ok(_) => println!("    ✓ Unexpectedly succeeded"),
            Err(e) => match e {
                QuarryError::TypeNotFound(_) => {
                    println!("    ❌ Type not found (expected)");
                    if !name.contains("::") {
                        println!("    💡 Tip: Use full module path like 'alloc::string::String'");
                    } else if name.contains("Option") {
                        println!("    💡 Note: Option is an enum, not a struct. Enum support is planned for future releases.");
                    }
                }
                other => println!("    ❌ Other error: {}", other),
            },
        }
        println!();
    }
}
