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
mod indexer;
mod service;

pub use handler::LanguageHandler;
pub use indexer::{FileIndexer, IndexEvent};
pub use service::LanguageService;
