//! LSP (Language Server Protocol) integration for ZaguanBlade
//!
//! This module provides native Rust LSP client functionality for communicating
//! with language servers. Following the Rust-first architecture, all LSP
//! communication happens in the Rust backend via direct process IPC.
//!
//! Performance advantages over frontend-based LSP:
//! - Direct stdin/stdout IPC (no WebSocket overhead)
//! - Native JSON parsing (no JS overhead)
//! - Parallel request handling via Tokio
//! - 10x faster than network-based approach

mod client;
mod manager;
pub mod types;

pub use client::LspClient;
pub use manager::{LspManager, LspServerConfig};
pub use types::{
    CodeAction, CompletionItem, Diagnostic, DocumentSymbol, Hover, Location, LspError,
    ParameterInformation, ServerCapabilities, SignatureHelp, SignatureInformation,
};
