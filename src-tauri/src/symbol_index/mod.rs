//! Symbol Index for ZaguanBlade
//!
//! Provides persistent storage and fast search for code symbols extracted
//! by tree-sitter. Uses SQLite with FTS5 for full-text search.
//!
//! Performance targets:
//! - Symbol search: <50ms
//! - File indexing: <100ms for 1000 lines
//! - Workspace index: <20s for 1000 files

mod search;
mod store;

pub use search::{SearchQuery, SearchResult};
pub use store::SymbolStore;
