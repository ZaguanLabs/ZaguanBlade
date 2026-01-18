//! LSP JSON-RPC client
//!
//! Handles low-level JSON-RPC 2.0 communication with language servers
//! via stdin/stdout pipes.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use super::types::{LspError, ServerCapabilities};

/// JSON-RPC request ID type
pub type RequestId = i64;

/// LSP Client for communicating with a language server
pub struct LspClient {
    /// Server process
    process: Child,
    /// Stdin for sending requests
    stdin: Mutex<ChildStdin>,
    /// Server capabilities after initialization
    pub capabilities: ServerCapabilities,
    /// Next request ID
    next_id: AtomicI64,
    /// Root URI of the workspace
    root_uri: String,
    /// Language ID (e.g., "typescript", "python")
    language_id: String,
    /// Cached diagnostics per file (updated via notifications)
    diagnostics: Arc<Mutex<HashMap<String, Vec<super::types::Diagnostic>>>>,
}

impl LspClient {
    /// Create a new LSP client by spawning a language server
    pub fn new(
        command: &str,
        args: &[&str],
        root_uri: &str,
        language_id: &str,
    ) -> Result<Self, LspError> {
        // Spawn the language server process
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LspError::SpawnFailed(format!("{}: {}", command, e)))?;

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| LspError::SpawnFailed("Failed to open stdin".to_string()))?;

        let client = Self {
            process,
            stdin: Mutex::new(stdin),
            capabilities: ServerCapabilities::default(),
            next_id: AtomicI64::new(1),
            root_uri: root_uri.to_string(),
            language_id: language_id.to_string(),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
        };

        Ok(client)
    }

    /// Initialize the LSP server (must be called before any other requests)
    pub fn initialize(&mut self) -> Result<(), LspError> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true,
                            "documentationFormat": ["markdown", "plaintext"]
                        }
                    },
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "synchronization": {
                        "didSave": true,
                        "didOpen": true,
                        "didClose": true
                    },
                    "publishDiagnostics": {
                        "relatedInformation": true
                    }
                },
                "workspace": {
                    "workspaceFolders": true
                }
            },
            "workspaceFolders": [{
                "uri": self.root_uri,
                "name": "workspace"
            }]
        });

        let response = self.send_request_sync("initialize", params)?;
        self.capabilities = ServerCapabilities::from_initialize_result(&response);

        // Send initialized notification
        self.send_notification("initialized", json!({}))?;

        Ok(())
    }

    /// Send a request and wait for response (synchronous)
    pub fn send_request_sync(&mut self, method: &str, params: Value) -> Result<Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.write_message(&request)?;

        // For now, we read response synchronously
        // In production, we would spawn a reader task
        self.read_response_sync(id)
    }

    /// Send a notification (no response expected)
    pub fn send_notification(&self, method: &str, params: Value) -> Result<(), LspError> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        self.write_message(&notification)
    }

    /// Open a text document
    pub fn did_open(&self, uri: &str, content: &str) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": self.language_id,
                    "version": 1,
                    "text": content
                }
            }),
        )
    }

    /// Notify of document changes
    pub fn did_change(&self, uri: &str, version: i32, content: &str) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": version
                },
                "contentChanges": [{
                    "text": content
                }]
            }),
        )
    }

    /// Close a text document
    pub fn did_close(&self, uri: &str) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didClose",
            json!({
                "textDocument": {
                    "uri": uri
                }
            }),
        )
    }

    /// Get completions at position
    pub fn completion(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<super::types::CompletionItem>, LspError> {
        if !self.capabilities.completion {
            return Ok(vec![]);
        }

        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let response = self.send_request_sync("textDocument/completion", params)?;

        // Response can be CompletionItem[] or CompletionList
        let items = if response.is_array() {
            response
        } else if let Some(items) = response.get("items") {
            items.clone()
        } else {
            return Ok(vec![]);
        };

        serde_json::from_value(items).map_err(LspError::from)
    }

    /// Get hover information at position
    pub fn hover(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<super::types::Hover>, LspError> {
        if !self.capabilities.hover {
            return Ok(None);
        }

        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let response = self.send_request_sync("textDocument/hover", params)?;

        if response.is_null() {
            return Ok(None);
        }

        serde_json::from_value(response).map_err(LspError::from)
    }

    /// Go to definition
    pub fn definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<super::types::Location>, LspError> {
        if !self.capabilities.definition {
            return Ok(vec![]);
        }

        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let response = self.send_request_sync("textDocument/definition", params)?;

        if response.is_null() {
            return Ok(vec![]);
        }

        // Response can be Location, Location[], or LocationLink[]
        if response.is_array() {
            serde_json::from_value(response).map_err(LspError::from)
        } else {
            // Single location
            let loc: super::types::Location = serde_json::from_value(response)?;
            Ok(vec![loc])
        }
    }

    /// Find references
    pub fn references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<super::types::Location>, LspError> {
        if !self.capabilities.references {
            return Ok(vec![]);
        }

        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        });

        let response = self.send_request_sync("textDocument/references", params)?;

        if response.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(response).map_err(LspError::from)
    }

    /// Get document symbols
    pub fn document_symbols(
        &mut self,
        uri: &str,
    ) -> Result<Vec<super::types::DocumentSymbol>, LspError> {
        if !self.capabilities.document_symbol {
            return Ok(vec![]);
        }

        let params = json!({
            "textDocument": { "uri": uri }
        });

        let response = self.send_request_sync("textDocument/documentSymbol", params)?;

        if response.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(response).map_err(LspError::from)
    }

    /// Get cached diagnostics for a file
    pub fn get_diagnostics(&self, uri: &str) -> Vec<super::types::Diagnostic> {
        self.diagnostics
            .lock()
            .unwrap()
            .get(uri)
            .cloned()
            .unwrap_or_default()
    }

    /// Shutdown the server gracefully
    pub fn shutdown(&mut self) -> Result<(), LspError> {
        // Send shutdown request
        let _ = self.send_request_sync("shutdown", json!(null));

        // Send exit notification
        let _ = self.send_notification("exit", json!(null));

        // Wait for process to exit
        let _ = self.process.wait();

        Ok(())
    }

    /// Write a JSON-RPC message to the server
    fn write_message(&self, message: &Value) -> Result<(), LspError> {
        let content = serde_json::to_string(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", content.len());

        let mut stdin = self.stdin.lock().unwrap();
        stdin.write_all(header.as_bytes())?;
        stdin.write_all(content.as_bytes())?;
        stdin.flush()?;

        Ok(())
    }

    /// Read a response synchronously (blocking)
    fn read_response_sync(&mut self, expected_id: RequestId) -> Result<Value, LspError> {
        // Clone Arc references before taking mutable borrow of stdout
        let diagnostics = Arc::clone(&self.diagnostics);

        let stdout = self
            .process
            .stdout
            .as_mut()
            .ok_or_else(|| LspError::IoError("stdout not available".to_string()))?;

        let mut reader = BufReader::new(stdout);

        loop {
            // Read headers
            let mut content_length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                let line = line.trim();

                if line.is_empty() {
                    break;
                }

                if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                    content_length = len_str
                        .parse()
                        .map_err(|_| LspError::ParseError("Invalid Content-Length".to_string()))?;
                }
            }

            if content_length == 0 {
                return Err(LspError::ParseError("Missing Content-Length".to_string()));
            }

            // Read content
            let mut content = vec![0u8; content_length];
            std::io::Read::read_exact(&mut reader, &mut content)?;

            let message: Value = serde_json::from_slice(&content)?;

            // Check if this is a response to our request
            if let Some(id) = message.get("id").and_then(|v| v.as_i64()) {
                if id == expected_id {
                    // Check for error
                    if let Some(error) = message.get("error") {
                        let code = error.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
                        let msg = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error")
                            .to_string();
                        return Err(LspError::RpcError { code, message: msg });
                    }

                    return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                }
            }

            // Handle notifications (like publishDiagnostics) inline to avoid borrow issues
            if message.get("id").is_none() {
                if let Some(method) = message.get("method").and_then(|v| v.as_str()) {
                    if method == "textDocument/publishDiagnostics" {
                        if let Some(params) = message.get("params") {
                            if let (Some(uri), Some(diags_value)) = (
                                params.get("uri").and_then(|v| v.as_str()),
                                params.get("diagnostics"),
                            ) {
                                if let Ok(diags) = serde_json::from_value(diags_value.clone()) {
                                    diagnostics.lock().unwrap().insert(uri.to_string(), diags);
                                }
                            }
                        }
                    } else {
                        eprintln!("[LSP] Notification: {}", method);
                    }
                }
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Try to shutdown gracefully
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_format() {
        let message = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let content = serde_json::to_string(&message).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", content.len());

        assert!(header.starts_with("Content-Length: "));
        assert!(header.ends_with("\r\n\r\n"));
    }
}
