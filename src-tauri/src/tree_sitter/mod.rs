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
mod symbol;

pub use parser::{
    ExtractionCapabilities, Language, LanguageCapability, ParserKind, SupportLevel,
    TreeSitterGrammar, TreeSitterParser,
};
pub(crate) use symbol::{
    collect_routes, extract_symbol_relationships_with_routes, extract_symbols_with_routes,
    stable_symbol_id,
};
pub use symbol::{
    extract_symbol_relationships, extract_symbols, Position, Range, Symbol, SymbolExtractor,
    SymbolRelationship, SymbolRelationshipType, SymbolType,
};
