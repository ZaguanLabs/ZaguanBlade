//! LSP type definitions and conversions
//!
//! Provides error types and capability tracking for LSP servers.

use serde::{Deserialize, Serialize};

/// Error types for LSP operations
#[derive(Debug, Clone)]
pub enum LspError {
    /// Server not found or not running
    ServerNotFound(String),
    /// Failed to spawn server process
    SpawnFailed(String),
    /// Server initialization failed
    InitializationFailed(String),
    /// Request timed out
    Timeout,
    /// JSON-RPC error from server
    RpcError { code: i32, message: String },
    /// Failed to parse response
    ParseError(String),
    /// Server shut down unexpectedly
    ServerShutdown,
    /// IO error during communication
    IoError(String),
    /// Unsupported language
    UnsupportedLanguage(String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspError::ServerNotFound(lang) => write!(f, "LSP server not found for: {}", lang),
            LspError::SpawnFailed(msg) => write!(f, "Failed to spawn LSP server: {}", msg),
            LspError::InitializationFailed(msg) => write!(f, "LSP initialization failed: {}", msg),
            LspError::Timeout => write!(f, "LSP request timed out"),
            LspError::RpcError { code, message } => {
                write!(f, "LSP RPC error {}: {}", code, message)
            }
            LspError::ParseError(msg) => write!(f, "Failed to parse LSP response: {}", msg),
            LspError::ServerShutdown => write!(f, "LSP server shut down unexpectedly"),
            LspError::IoError(msg) => write!(f, "LSP IO error: {}", msg),
            LspError::UnsupportedLanguage(lang) => write!(f, "Unsupported language: {}", lang),
        }
    }
}

impl std::error::Error for LspError {}

impl From<std::io::Error> for LspError {
    fn from(err: std::io::Error) -> Self {
        LspError::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for LspError {
    fn from(err: serde_json::Error) -> Self {
        LspError::ParseError(err.to_string())
    }
}

/// Tracked server capabilities
///
/// We track which capabilities the server advertised during initialization
/// to know which features are available.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    /// Server supports textDocument/completion
    pub completion: bool,
    /// Server supports textDocument/hover
    pub hover: bool,
    /// Server supports textDocument/definition
    pub definition: bool,
    /// Server supports textDocument/references
    pub references: bool,
    /// Server supports textDocument/documentSymbol
    pub document_symbol: bool,
    /// Server supports textDocument/formatting
    pub formatting: bool,
    /// Server supports textDocument/rename
    pub rename: bool,
    /// Server supports textDocument/codeAction
    pub code_action: bool,
    /// Server supports textDocument/signatureHelp
    pub signature_help: bool,
    /// Server supports workspace/symbol
    pub workspace_symbol: bool,
    /// Server supports textDocument/publishDiagnostics
    pub diagnostics: bool,
}

impl ServerCapabilities {
    /// Create capabilities from LSP InitializeResult
    pub fn from_initialize_result(result: &serde_json::Value) -> Self {
        let caps = result.get("capabilities").unwrap_or(result);

        Self {
            completion: caps.get("completionProvider").is_some(),
            hover: caps
                .get("hoverProvider")
                .map(|v| !v.is_null())
                .unwrap_or(false),
            definition: caps
                .get("definitionProvider")
                .map(|v| !v.is_null())
                .unwrap_or(false),
            references: caps
                .get("referencesProvider")
                .map(|v| !v.is_null())
                .unwrap_or(false),
            document_symbol: caps
                .get("documentSymbolProvider")
                .map(|v| !v.is_null())
                .unwrap_or(false),
            formatting: caps
                .get("documentFormattingProvider")
                .map(|v| !v.is_null())
                .unwrap_or(false),
            rename: caps.get("renameProvider").is_some(),
            code_action: caps.get("codeActionProvider").is_some(),
            signature_help: caps.get("signatureHelpProvider").is_some(),
            workspace_symbol: caps
                .get("workspaceSymbolProvider")
                .map(|v| !v.is_null())
                .unwrap_or(false),
            diagnostics: true, // Diagnostics are always pushed via notifications
        }
    }
}

/// LSP Position (0-indexed line and character)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// LSP Range
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// LSP Diagnostic severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// LSP Diagnostic
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<DiagnosticSeverity>,
    pub code: Option<serde_json::Value>,
    pub source: Option<String>,
    pub message: String,
}

/// LSP CompletionItem
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
}

/// LSP Hover result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hover {
    pub contents: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// LSP Location
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// LSP DocumentSymbol
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: i32,
    pub range: Range,
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DocumentSymbol>,
}

/// LSP SignatureHelp
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelp {
    pub signatures: Vec<SignatureInformation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_signature: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_parameter: Option<u32>,
}

/// LSP SignatureInformation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInformation {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParameterInformation>,
}

/// LSP ParameterInformation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterInformation {
    /// The label can be a string or [start, end] offsets into the signature label
    pub label: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<serde_json::Value>,
}

/// LSP TextEdit
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// LSP WorkspaceEdit
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEdit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<std::collections::HashMap<String, Vec<TextEdit>>>,
}

/// LSP CodeAction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAction {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub is_preferred: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<WorkspaceEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_parsing() {
        let result = serde_json::json!({
            "capabilities": {
                "completionProvider": {
                    "triggerCharacters": ["."]
                },
                "hoverProvider": true,
                "definitionProvider": true,
                "referencesProvider": true
            }
        });

        let caps = ServerCapabilities::from_initialize_result(&result);
        assert!(caps.completion);
        assert!(caps.hover);
        assert!(caps.definition);
        assert!(caps.references);
        assert!(!caps.formatting);
    }

    #[test]
    fn test_position_serialization() {
        let pos = Position::new(10, 5);
        let json = serde_json::to_string(&pos).unwrap();
        assert_eq!(json, r#"{"line":10,"character":5}"#);
    }
}
