//! Language Service for ZaguanBlade
//!
//! Unified service that coordinates tree-sitter parsing, symbol extraction,
//! SQLite storage, and LSP integration. This is the main entry point for
//! all language-related operations.
//!
//! Architecture:
//! - Tree-sitter: Fast AST parsing and symbol extraction
//! - Symbol Index: Persistent SQLite storage with FTS5 search
//! - LSP Manager: Optional LSP server integration for enhanced features

mod indexer;
mod service;

pub use indexer::{FileIndexer, IndexEvent};
pub use service::LanguageService;
