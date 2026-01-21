//! LSP Server Manager
//!
//! Manages lifecycle and configuration of multiple language servers.
//! Each language can have its own server with specific configuration.

use std::collections::HashMap;
use std::path::Path;

use super::client::LspClient;
use super::types::{
    CompletionItem, Diagnostic, DocumentSymbol, Hover, Location, LspError,
    ServerCapabilities, WorkspaceEdit,
};

/// Configuration for spawning a language server
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    /// Command to run (e.g., "typescript-language-server")
    pub command: String,
    /// Command line arguments
    pub args: Vec<String>,
    /// Language ID for textDocument/didOpen
    pub language_id: String,
    /// File extensions this server handles
    pub extensions: Vec<String>,
}

impl LspServerConfig {
    /// Create config for TypeScript/JavaScript
    pub fn typescript() -> Self {
        Self {
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            language_id: "typescript".to_string(),
            extensions: vec![
                "ts".to_string(),
                "tsx".to_string(),
                "js".to_string(),
                "jsx".to_string(),
            ],
        }
    }

    /// Create config for Python (pylsp)
    pub fn python() -> Self {
        Self {
            command: "pylsp".to_string(),
            args: vec![],
            language_id: "python".to_string(),
            extensions: vec!["py".to_string()],
        }
    }

    /// Create config for Rust (rust-analyzer)
    pub fn rust() -> Self {
        Self {
            command: "rust-analyzer".to_string(),
            args: vec![],
            language_id: "rust".to_string(),
            extensions: vec!["rs".to_string()],
        }
    }
}

/// Language server manager
///
/// Manages multiple language servers, one per language.
/// Automatically routes requests to the appropriate server based on file extension.
pub struct LspManager {
    /// Active language servers
    servers: HashMap<String, LspClient>,
    /// Server configurations
    configs: HashMap<String, LspServerConfig>,
    /// Extension to language mapping
    extension_map: HashMap<String, String>,
    /// Workspace root URI
    workspace_root: String,
}

impl LspManager {
    /// Create a new LSP manager for a workspace
    pub fn new(workspace_root: &str) -> Self {
        let mut manager = Self {
            servers: HashMap::new(),
            configs: HashMap::new(),
            extension_map: HashMap::new(),
            workspace_root: if workspace_root.starts_with("file://") {
                workspace_root.to_string()
            } else {
                format!("file://{}", workspace_root)
            },
        };

        // Register default server configurations
        manager.register_config("typescript", LspServerConfig::typescript());
        manager.register_config("python", LspServerConfig::python());
        manager.register_config("rust", LspServerConfig::rust());

        manager
    }

    /// Register a server configuration
    pub fn register_config(&mut self, language: &str, config: LspServerConfig) {
        for ext in &config.extensions {
            self.extension_map.insert(ext.clone(), language.to_string());
        }
        self.configs.insert(language.to_string(), config);
    }

    /// Get the language for a file path
    pub fn language_for_file(&self, file_path: &str) -> Option<String> {
        let ext = Path::new(file_path).extension().and_then(|e| e.to_str())?;
        self.extension_map.get(ext).cloned()
    }

    /// Start a language server if not already running
    pub fn ensure_server(&mut self, language: &str) -> Result<(), LspError> {
        if self.servers.contains_key(language) {
            return Ok(());
        }

        let config = self
            .configs
            .get(language)
            .ok_or_else(|| LspError::UnsupportedLanguage(language.to_string()))?
            .clone();

        eprintln!("[LSP] Starting {} server: {}", language, config.command);

        let args: Vec<&str> = config.args.iter().map(|s| s.as_str()).collect();
        let mut client = LspClient::new(
            &config.command,
            &args,
            &self.workspace_root,
            &config.language_id,
        )?;

        client.initialize()?;

        eprintln!("[LSP] {} server initialized", language);
        self.servers.insert(language.to_string(), client);

        Ok(())
    }

    /// Stop a language server
    pub fn stop_server(&mut self, language: &str) -> Result<(), LspError> {
        if let Some(mut client) = self.servers.remove(language) {
            client.shutdown()?;
            eprintln!("[LSP] {} server stopped", language);
        }
        Ok(())
    }

    /// Stop all servers
    pub fn stop_all(&mut self) {
        let languages: Vec<_> = self.servers.keys().cloned().collect();
        for lang in languages {
            let _ = self.stop_server(&lang);
        }
    }

    /// Get server capabilities for a language
    pub fn capabilities(&self, language: &str) -> Option<&ServerCapabilities> {
        self.servers.get(language).map(|c| &c.capabilities)
    }

    /// Check if a server is running
    pub fn is_running(&self, language: &str) -> bool {
        self.servers.contains_key(language)
    }

    /// Open a document in the appropriate server
    pub fn did_open(&mut self, file_path: &str, content: &str) -> Result<(), LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .did_open(&uri, content)
    }

    /// Notify document change
    pub fn did_change(
        &mut self,
        file_path: &str,
        version: i32,
        content: &str,
    ) -> Result<(), LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        if !self.servers.contains_key(&language) {
            return Ok(()); // Server not started, ignore
        }

        let uri = path_to_uri(file_path);
        self.servers
            .get(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .did_change(&uri, version, content)
    }

    /// Close a document
    pub fn did_close(&mut self, file_path: &str) -> Result<(), LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        if !self.servers.contains_key(&language) {
            return Ok(());
        }

        let uri = path_to_uri(file_path);
        self.servers
            .get(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .did_close(&uri)
    }

    /// Get completions at position
    pub fn completion(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<CompletionItem>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .completion(&uri, line, character)
    }

    /// Get hover information
    pub fn hover(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Hover>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .hover(&uri, line, character)
    }

    /// Go to definition
    pub fn definition(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .definition(&uri, line, character)
    }

    /// Find references
    pub fn references(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .references(&uri, line, character, include_declaration)
    }

    /// Get document symbols
    pub fn document_symbols(&mut self, file_path: &str) -> Result<Vec<DocumentSymbol>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .document_symbols(&uri)
    }

    /// Get cached diagnostics for a file
    pub fn get_diagnostics(&self, file_path: &str) -> Vec<Diagnostic> {
        let language = match self.language_for_file(file_path) {
            Some(l) => l,
            None => return vec![],
        };

        let uri = path_to_uri(file_path);
        self.servers
            .get(&language)
            .map(|c| c.get_diagnostics(&uri))
            .unwrap_or_default()
    }

    /// Get signature help (parameter hints)
    pub fn signature_help(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<super::types::SignatureHelp>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .signature_help(&uri, line, character)
    }

    /// Rename a symbol
    pub fn rename(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);
        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .rename(&uri, line, character, new_name)
    }

    /// Get code actions (quick fixes, refactorings)
    pub fn code_actions(
        &mut self,
        file_path: &str,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> Result<Vec<super::types::CodeAction>, LspError> {
        let language = self
            .language_for_file(file_path)
            .ok_or_else(|| LspError::UnsupportedLanguage(file_path.to_string()))?;

        self.ensure_server(&language)?;

        let uri = path_to_uri(file_path);

        // Get any cached diagnostics for this range to include in context
        let diagnostics = self.get_diagnostics(file_path);
        let relevant_diagnostics: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.range.start.line >= start_line && d.range.end.line <= end_line)
            .collect();

        self.servers
            .get_mut(&language)
            .ok_or(LspError::ServerNotFound(language))?
            .code_actions(
                &uri,
                start_line,
                start_char,
                end_line,
                end_char,
                &relevant_diagnostics,
            )
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// Convert a file path to a file:// URI
fn path_to_uri(path: &str) -> String {
    if path.starts_with("file://") {
        path.to_string()
    } else {
        format!("file://{}", path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        let manager = LspManager::new("/tmp/workspace");

        assert_eq!(
            manager.language_for_file("main.ts"),
            Some("typescript".to_string())
        );
        assert_eq!(
            manager.language_for_file("app.tsx"),
            Some("typescript".to_string())
        );
        assert_eq!(
            manager.language_for_file("script.js"),
            Some("typescript".to_string())
        );
        assert_eq!(
            manager.language_for_file("main.py"),
            Some("python".to_string())
        );
        assert_eq!(
            manager.language_for_file("lib.rs"),
            Some("rust".to_string())
        );
        assert_eq!(manager.language_for_file("data.json"), None);
    }

    #[test]
    fn test_path_to_uri() {
        assert_eq!(
            path_to_uri("/home/user/project/main.ts"),
            "file:///home/user/project/main.ts"
        );
        assert_eq!(path_to_uri("file:///already/uri"), "file:///already/uri");
    }

    #[test]
    fn test_typescript_config() {
        let config = LspServerConfig::typescript();
        assert_eq!(config.command, "typescript-language-server");
        assert!(config.extensions.contains(&"ts".to_string()));
        assert!(config.extensions.contains(&"tsx".to_string()));
    }
}
