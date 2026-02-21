//! Wiki Text Benchmarking Tool
//!
//! This binary measures the performance of our text processing pipeline on large text files,
//! like Wikipedia abstracts or full article dumps. It's designed to give realistic throughput
//! numbers for production-like workloads.
//!
//! ## What It Benchmarks
//!
//! The tool measures three stages of the pipeline:
//!
//! 1. **Normalization**: Converting raw text to lowercase, collapsing whitespace
//! 2. **Tokenization**: Splitting normalized text into individual tokens
//! 3. **Full Pipeline**: Normalization + tokenization together (materialized)
//!
//! ## Usage
//!
//! ```bash
//! # Benchmark normalization only
//! ./target/release/wiki_bench /path/to/wiki.txt normalize
//!
//! # Benchmark tokenization only (input should be pre-normalized)
//! ./target/release/wiki_bench /path/to/wiki.txt tokenize
//!
//! # Benchmark full pipeline
//! ./target/release/wiki_bench /path/to/wiki.txt pipeline
//!
//! # Run all three modes
//! ./target/release/wiki_bench /path/to/wiki.txt all
//!
//! # Specify a different field (title, body, tag)
//! ./target/release/wiki_bench /path/to/wiki.txt pipeline title
//! ```
//!
//! ## Output
//!
//! The benchmark prints:
//! - **Elapsed time**: How long the operation took
//! - **Throughput**: GiB/second processed
//! - **Token count**: Number of tokens produced (for tokenize/pipeline modes)
//! - **Tokens/sec**: Token generation rate
//!
//! ## Example Output
//!
//! ```text
//! === Pipeline (materialized) ===
//! --------------------------------
//! Mode        : Pipeline
//! Elapsed     : 0.452 s
//! Throughput  : 2.18 GiB/s
//! Tokens      : 154_892_341
//! Tokens/sec  : 342_654_789
//! --------------------------------
//! ```
//!
//! ## Tips for Accurate Results
//!
//! - Run with `--release` flag (this binary should be built in release mode)
//! - Use a large input file (100MB+) for stable measurements
//! - Consider using `taskset` to pin to a specific CPU core
//! - Disable turbo boost and CPU frequency scaling for consistent results
//! - Disable ASLR if you want perfectly reproducible measurements

use std::env;
use std::fs;
use std::time::{Duration, Instant};

use lattice_core::analyzer::normalizer::TextNormalizer;
use lattice_core::analyzer::tokenizer::{Field, Tokenizer};

const WARMUP_RUNS: usize = 1;
const MEASURE_RUNS: usize = 5;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: wiki_bench <path> [field]");
        std::process::exit(1);
    }

    let path = &args[1];

    let field = match args.get(2).map(String::as_str) {
        Some("title") => Field::Title,
        Some("tag") => Field::Tag,
        _ => Field::Body,
    };

    println!("Loading file...");
    let bytes = fs::read(path)?;
    let input = std::str::from_utf8(&bytes).expect("input must be valid UTF-8");

    println!("File size: {}", fmt_bytes(input.len() as u64));
    println!("Field:     {:?}\n", field);

    bench_normalize(input);
    bench_tokenize(input, field);
    bench_pipeline(input, field);

    Ok(())
}

fn bench_normalize(input: &str) {
    let normalizer = TextNormalizer::default();
    let mut out = String::with_capacity(input.len());

    println!("=== Normalize ===");

    warmup(|| {
        normalizer.normalize_into(input, &mut out);
    });

    let elapsed = measure(|| {
        normalizer.normalize_into(input, &mut out);
    });

    print_perf("Normalize", input.len(), elapsed, 0);
}

fn bench_tokenize(input: &str, field: Field) {
    let tokenizer = Tokenizer::new(field);

    println!("=== Tokenize ===");

    warmup(|| {
        let mut sink = 0u64;
        tokenizer.tokenize(input, |_t, _f, _p| {
            sink += 1;
        });
        std::hint::black_box(sink);
    });

    let mut tokens = 0u64;
    let elapsed = measure(|| {
        let mut local = 0u64;
        tokenizer.tokenize(input, |_t, _f, _p| {
            local += 1;
        });
        tokens = local;
        std::hint::black_box(tokens);
    });

    print_perf("Tokenize", input.len(), elapsed, tokens);
}

fn bench_pipeline(input: &str, field: Field) {
    let normalizer = TextNormalizer::default();
    let tokenizer = Tokenizer::new(field);
    let mut norm_buf = String::with_capacity(input.len());

    println!("=== Pipeline (materialized) ===");

    warmup(|| {
        normalizer.normalize_into(input, &mut norm_buf);
        let mut sink = 0u64;
        tokenizer.tokenize(&norm_buf, |_t, _f, _p| {
            sink += 1;
        });
        std::hint::black_box(sink);
    });

    let mut tokens = 0u64;
    let elapsed = measure(|| {
        normalizer.normalize_into(input, &mut norm_buf);

        let mut local = 0u64;
        tokenizer.tokenize(&norm_buf, |_t, _f, _p| {
            local += 1;
        });

        tokens = local;
        std::hint::black_box(tokens);
    });

    print_perf("Pipeline", input.len(), elapsed, tokens);
}

fn warmup<F: FnMut()>(mut f: F) {
    for _ in 0..WARMUP_RUNS {
        f();
    }
}

fn measure<F: FnMut()>(mut f: F) -> Duration {
    let mut total = Duration::ZERO;

    for _ in 0..MEASURE_RUNS {
        let start = Instant::now();
        f();
        total += start.elapsed();
    }

    total / MEASURE_RUNS as u32
}

fn print_perf(label: &str, input_bytes: usize, elapsed: Duration, tokens: u64) {
    let secs = elapsed.as_secs_f64();
    let gib = input_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    println!("--------------------------------");
    println!("Mode        : {}", label);
    println!("Elapsed     : {:.3} s", secs);
    println!("Throughput  : {:.3} GiB/s", gib / secs);

    if tokens > 0 {
        println!("Tokens      : {}", fmt_count(tokens));
        println!("Tokens/sec  : {}", fmt_count((tokens as f64 / secs) as u64));
    }

    println!("--------------------------------\n");
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024 * 1024 {
        format!("{:.2} MiB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.2} KiB", b as f64 / 1024.0)
    } else {
        format!("{} B", b)
    }
}

fn fmt_count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);

    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push('_');
        }
        out.push(ch);
    }

    out.chars().rev().collect()
}
