//! Typed, model-facing wire types for symbol tools.
//!
//! Storage and parser coordinates are zero-based. The public tool contract is
//! one-based for lines and zero-based for characters, so conversion belongs at
//! this boundary rather than being repeated in individual tools.

use serde::Serialize;

use crate::tree_sitter::Symbol;

#[derive(Debug, Serialize)]
pub(crate) struct WirePosition {
    line: u32,
    character: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireRange {
    start: WirePosition,
    end: WirePosition,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireSymbol<'a> {
    id: &'a str,
    name: &'a str,
    qualified_name: &'a str,
    symbol_type: String,
    file_path: &'a str,
    range: WireRange,
    byte_offset: usize,
    byte_length: usize,
    parent_id: Option<&'a str>,
    signature: Option<&'a str>,
    has_docstring: bool,
    content_hash: &'a str,
}

impl<'a> From<&'a Symbol> for WireSymbol<'a> {
    fn from(symbol: &'a Symbol) -> Self {
        Self {
            id: &symbol.id,
            name: &symbol.name,
            qualified_name: &symbol.qualified_name,
            symbol_type: symbol.symbol_type.to_string(),
            file_path: &symbol.file_path,
            range: WireRange {
                start: WirePosition {
                    line: model_line(symbol.range.start.line),
                    character: symbol.range.start.character,
                },
                end: WirePosition {
                    line: model_line(symbol.range.end.line),
                    character: symbol.range.end.character,
                },
            },
            byte_offset: symbol.byte_offset,
            byte_length: symbol.byte_length,
            parent_id: symbol.parent_id.as_deref(),
            signature: symbol.signature.as_deref(),
            has_docstring: symbol
                .docstring
                .as_deref()
                .is_some_and(|doc| !doc.is_empty()),
            content_hash: &symbol.content_hash,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct WireRelationshipEdge {
    pub source_symbol_id: String,
    pub target_symbol_id: Option<String>,
    pub target_name: String,
    pub relationship_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traversal_direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_confidence: Option<f32>,
    pub observation: serde_json::Value,
    pub resolution: serde_json::Value,
    /// Qualified Rust call: byte offset of the call target for exact call-site
    /// identity. Omitted when the observation has no byte offset (non-Rust or
    /// legacy rows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<u32>,
    /// Qualified Rust call: normalized qualifier segments before the terminal
    /// name (e.g. `["crate", "store", "SymbolStore"]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier_segments: Option<Vec<String>>,
    /// Qualified Rust call: syntactic call form — `bare`, `receiver`,
    /// `associated`, `self_path`, `crate_path`, `module_path`, or `ufcs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_form: Option<String>,
    /// Qualified Rust call: human-readable observed target reconstructed from
    /// qualifier segments + terminal (e.g. `crate::store::SymbolStore::new`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_qualified_target: Option<String>,
    /// Qualified Rust call: stable unresolved reason category when no target
    /// was assigned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_reason: Option<String>,
}

pub(crate) const fn model_line(zero_based_line: u32) -> u32 {
    zero_based_line.saturating_add(1)
}
