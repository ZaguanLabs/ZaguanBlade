//! Tree-sitter integration for ZaguanBlade
//!
//! This module provides native Rust tree-sitter parsing for code analysis.
//! Following the Rust-first architecture principle, all parsing and symbol
//! extraction happens here in the backend for maximum performance.
//!
//! Performance targets:
//! - Parse time: <5ms for 1000 lines (vs 50ms in WASM/JS)
//! - Symbol extraction: <3ms (vs 30ms in JS)
//! - Memory: Efficient native allocation (vs browser limits)

mod parser;
mod query;
mod symbol;

pub use parser::{Language, TreeSitterParser};
pub use query::QueryManager;
pub use symbol::{extract_symbols, Position, Range, Symbol, SymbolExtractor, SymbolType};
