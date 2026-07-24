//! Language Service for ZaguanBlade
//!
//! Unified service that coordinates tree-sitter parsing, symbol extraction,
//! and SQLite storage. This is the main entry point for all language-related
//! operations.
//!
//! Architecture:
//! - Tree-sitter: Fast AST parsing and symbol extraction
//! - Symbol Index: Persistent SQLite storage with FTS5 search

pub mod handler;
mod rust_project;
mod service;

pub use handler::LanguageHandler;
pub use service::{
    extract_scanner_symbols, modelled_relationship_kinds, ArchitectureBridgeModule,
    ArchitectureCommunity, ArchitectureEdge,
    ArchitectureModule, ArchitectureSnapshot, IndexDiscoverySnapshot, IndexHealthSnapshot,
    IndexHealthStatus, IndexLanguageCount, IndexSchemaCount, IndexSchemaLanguageCount,
    IndexSchemaSnapshot, IndexSchemaTotals, IndexSkipCount, IndexTimingSnapshot, LanguageService,
    RelatedSymbol, SymbolGraph, SymbolPath, SymbolPathEdge, SymbolTrace, SymbolTraceDirection,
    SymbolTraceEdge, SymbolTraceNode,
};
