//! Advanced Usage Example for Quarry
//!
//! This example demonstrates advanced features and best practices for using Quarry:
//! - Cache management and statistics
//! - Bulk analysis of multiple types
//! - Performance optimization techniques
//! - Comprehensive error handling and recovery

use quarry::{
    cache_stats, clear_stdlib_cache, init_stdlib_cache, is_stdlib_struct, list_stdlib_structs,
    mine_struct_info,
};
use std::collections::HashMap;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize detailed logging
    env_logger::init();

    println!("🚀 Quarry - Advanced Usage Example");
    println!("====================================\n");

    // Example 1: Cache Management
    cache_management_demo()?;

    println!("\n{}\n", "═".repeat(60));

    // Example 2: Bulk Analysis
    bulk_analysis_demo()?;

    println!("\n{}\n", "═".repeat(60));

    // Example 3: Type Discovery
    type_discovery_demo()?;

    println!("\n{}\n", "═".repeat(60));

    // Example 4: Performance Analysis
    performance_analysis_demo()?;

    println!("\n✨ Advanced usage examples completed!");
    Ok(())
}

/// Demonstrates cache management features
fn cache_management_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🗄️  Cache Management Demo");
    println!("========================\n");

    // Check initial cache state
    let (count, initialized) = cache_stats()?;
    println!("📊 Initial cache state:");
    println!("   • Count: {} types", count);
    println!("   • Initialized: {}", initialized);

    // Manually initialize cache
    println!("\n⚡ Initializing cache manually...");
    let start = Instant::now();
    init_stdlib_cache()?;
    let duration = start.elapsed();
    println!("   ✓ Cache initialized in {:?}", duration);

    // Check cache state after initialization
    let (count, initialized) = cache_stats()?;
    println!("\n📊 Post-initialization cache state:");
    println!("   • Count: {} types", count);
    println!("   • Initialized: {}", initialized);

    // Demonstrate fast lookups after cache is warm
    println!("\n🏃 Testing fast lookups with warm cache:");
    let test_types = [
        "alloc::string::String",
        "alloc::vec::Vec",
        "std::collections::HashMap",
    ];

    for type_name in &test_types {
        let start = Instant::now();
        let exists = is_stdlib_struct(type_name);
        let duration = start.elapsed();
        println!("   • {} -> {} ({:?})", type_name, exists, duration);
    }

    // Clear cache demonstration
    println!("\n🧹 Clearing cache...");
    clear_stdlib_cache();
    let (count, initialized) = cache_stats()?;
    println!("   ✓ Cache cleared");
    println!("   • Count: {} types", count);
    println!("   • Initialized: {}", initialized);

    Ok(())
}

/// Demonstrates bulk analysis of multiple types
fn bulk_analysis_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Bulk Analysis Demo");
    println!("====================\n");

    println!("ℹ️  Note: Some types may fail because they are enums (not yet supported)");
    println!("   Enum support is planned for future releases.\n");

    // Define types to analyze across different crates
    let types_to_analyze = vec![
        // Core types (note: some may be enums and will fail)
        (
            "Core Types",
            vec![
                "core::mem::manually_drop::ManuallyDrop",
                "core::marker::PhantomData", 
                "core::time::Duration",
                "core::ptr::non_null::NonNull",
            ],
        ),
        // Alloc types
        (
            "Allocation Types",
            vec![
                "alloc::string::String",
                "alloc::vec::Vec",
                "alloc::boxed::Box",
                "alloc::rc::Rc",
            ],
        ),
        // Std collection types
        (
            "Collection Types",
            vec![
                "std::collections::HashMap",
                "std::collections::BTreeMap",
                "std::collections::HashSet",
                "std::collections::VecDeque",
            ],
        ),
    ];

    let mut analysis_results = HashMap::new();
    let mut total_analyzed = 0;
    let mut total_errors = 0;

    for (category, types) in types_to_analyze {
        println!("📂 Analyzing {} ({} types):", category, types.len());

        for type_name in types {
            match mine_struct_info(type_name) {
                Ok(info) => {
                    let field_count = info.fields.len();
                    let struct_type = if info.is_unit_struct {
                        "unit"
                    } else if info.is_tuple_struct {
                        "tuple"
                    } else {
                        "named"
                    };

                    println!(
                        "   ✓ {} -> {} fields, {} struct",
                        info.simple_name, field_count, struct_type
                    );

                    analysis_results.insert(type_name.to_string(), info);
                    total_analyzed += 1;
                }
                Err(e) => {
                    println!("   ❌ {} -> Error: {}", type_name, e);
                    total_errors += 1;
                }
            }
        }
        println!();
    }

    // Summary statistics
    println!("📈 Bulk Analysis Summary:");
    println!("   • Total analyzed: {}", total_analyzed);
    println!("   • Total errors: {}", total_errors);
    println!(
        "   • Success rate: {:.1}%",
        (total_analyzed as f64 / (total_analyzed + total_errors) as f64) * 100.0
    );

    // Find types with most fields
    if !analysis_results.is_empty() {
        let mut field_counts: Vec<_> = analysis_results
            .iter()
            .map(|(name, info)| (name, info.fields.len()))
            .collect();
        field_counts.sort_by(|a, b| b.1.cmp(&a.1));

        println!("\n🏆 Types with most fields:");
        for (name, count) in field_counts.iter().take(3) {
            println!("   • {}: {} fields", name, count);
        }
    }

    Ok(())
}

/// Demonstrates type discovery features
fn type_discovery_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Type Discovery Demo");
    println!("=====================\n");

    println!("📋 Discovering all available standard library structs...");
    let start = Instant::now();
    let all_structs = list_stdlib_structs()?;
    let duration = start.elapsed();

    println!(
        "   ✓ Found {} struct types in {:?}",
        all_structs.len(),
        duration
    );

    // Categorize by crate
    let mut crate_counts = HashMap::new();
    for struct_name in &all_structs {
        if let Some(crate_name) = struct_name.split("::").next() {
            *crate_counts.entry(crate_name.to_string()).or_insert(0) += 1;
        }
    }

    println!("\n📊 Types by crate:");
    let mut sorted_crates: Vec<_> = crate_counts.iter().collect();
    sorted_crates.sort_by(|a, b| b.1.cmp(a.1));

    for (crate_name, count) in sorted_crates {
        println!("   • {}: {} types", crate_name, count);
    }

    // Show some examples from each major crate
    println!("\n📝 Sample types from each crate:");
    for crate_name in ["std", "alloc", "core"] {
        let examples: Vec<_> = all_structs
            .iter()
            .filter(|s| s.starts_with(&format!("{}::", crate_name)))
            .take(3)
            .collect();

        if !examples.is_empty() {
            println!("   {} examples:", crate_name);
            for example in examples {
                println!("     • {}", example);
            }
        }
    }

    // Test the is_stdlib_struct function
    println!("\n🎯 Testing struct existence checks:");
    let test_cases = vec![
        ("alloc::string::String", true),
        ("std::collections::HashMap", true),
        ("core::mem::manually_drop::ManuallyDrop", true),
        ("core::option::Option", false), // This is an enum, not a struct
        ("my::custom::Type", false),
        ("String", false), // Short name should fail
    ];

    for (type_name, expected) in test_cases {
        let exists = is_stdlib_struct(type_name);
        let status = if exists == expected { "✓" } else { "❌" };
        println!(
            "   {} {} -> {} (expected: {})",
            status, type_name, exists, expected
        );
    }

    Ok(())
}

/// Demonstrates performance characteristics
fn performance_analysis_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚡ Performance Analysis Demo");
    println!("===========================\n");

    // Warm up the cache first
    println!("🔥 Warming up cache...");
    init_stdlib_cache()?;

    // Test query performance with warm cache
    println!("\n🏃 Testing query performance (warm cache):");
    let test_types = vec![
        "alloc::string::String",
        "alloc::vec::Vec",
        "std::collections::HashMap",
        "core::mem::manually_drop::ManuallyDrop",
        "std::collections::BTreeMap",
    ];

    let mut total_time = std::time::Duration::new(0, 0);

    for type_name in &test_types {
        let start = Instant::now();
        let result = mine_struct_info(type_name);
        let duration = start.elapsed();
        total_time += duration;

        match result {
            Ok(info) => println!(
                "   ✓ {} -> {} fields in {:?}",
                info.simple_name,
                info.fields.len(),
                duration
            ),
            Err(e) => println!("   ❌ {} -> Error in {:?}: {}", type_name, duration, e),
        }
    }

    let avg_time = total_time / test_types.len() as u32;
    println!("\n📊 Performance Summary:");
    println!("   • Total queries: {}", test_types.len());
    println!("   • Total time: {:?}", total_time);
    println!("   • Average time per query: {:?}", avg_time);

    // Test batch existence checks
    println!("\n🚄 Testing batch existence checks:");
    let start = Instant::now();
    let mut found_count = 0;

    for type_name in &test_types {
        if is_stdlib_struct(type_name) {
            found_count += 1;
        }
    }

    let batch_duration = start.elapsed();
    println!(
        "   ✓ Checked {} types in {:?}",
        test_types.len(),
        batch_duration
    );
    println!("   • Found: {}/{}", found_count, test_types.len());
    println!(
        "   • Average per check: {:?}",
        batch_duration / test_types.len() as u32
    );

    // Compare with cold cache performance
    println!("\n🧊 Testing with cold cache:");
    clear_stdlib_cache();

    let start = Instant::now();
    let result = mine_struct_info("alloc::string::String");
    let cold_duration = start.elapsed();

    match result {
        Ok(_) => println!("   ✓ First query (cold cache): {:?}", cold_duration),
        Err(e) => println!("   ❌ Cold cache error: {}", e),
    }

    println!("\n💡 Performance Tips:");
    println!("   • Call init_stdlib_cache() early for better performance");
    println!("   • Use is_stdlib_struct() for fast existence checks");
    println!("   • Cache initialization is one-time cost, subsequent queries are fast");
    println!("   • Consider pre-warming cache in long-running applications");

    Ok(())
}
