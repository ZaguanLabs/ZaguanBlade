//! Blade Protocol Handler for Language Service
//!
//! Handles `LanguageIntent`s and dispatches them to the `LanguageService`
//! or `SymbolStore`, returning appropriate `LanguageEvent`s.

use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::blade_protocol::{
    BladeError, BladeEvent, BladeEventEnvelope, BladeResult, CodeAction, CompletionItem,
    LanguageDiagnostic, LanguageDocumentSymbol, LanguageEvent, LanguageIntent, LanguageLocation,
    LanguagePosition, LanguageRange, LanguageSymbol, LanguageTextEdit, LanguageWorkspaceEdit,
    ParameterInfo, SignatureInfo,
};
use crate::language_service::LanguageService;
use crate::tree_sitter::SymbolType;

/// Handler for language intents
pub struct LanguageHandler {
    service: Arc<LanguageService>,
}

impl LanguageHandler {
    /// Create a new language handler
    pub fn new(service: Arc<LanguageService>) -> Self {
        Self { service }
    }

    /// Handle a language intent
    pub async fn handle(
        &self,
        intent: LanguageIntent,
        intent_id: Uuid,
    ) -> BladeResult<Option<BladeEventEnvelope>> {
        let event_payload = match intent {
            LanguageIntent::IndexFile { file_path } => {
                let symbols =
                    self.service
                        .index_file(&file_path)
                        .map_err(|e| BladeError::Internal {
                            trace_id: Uuid::new_v4().to_string(),
                            message: format!("Parsing failed: {}", e),
                        })?;

                LanguageEvent::FileIndexed {
                    file_path,
                    symbol_count: symbols.len(),
                }
            }
            LanguageIntent::IndexWorkspace => {
                let stats =
                    self.service
                        .index_directory(".")
                        .map_err(|e| BladeError::Internal {
                            trace_id: Uuid::new_v4().to_string(),
                            message: format!("Indexing failed: {}", e),
                        })?;

                LanguageEvent::WorkspaceIndexed {
                    file_count: stats.files_indexed,
                    symbol_count: stats.symbols_extracted,
                    duration_ms: stats.duration_ms,
                }
            }
            LanguageIntent::SearchSymbols {
                query,
                file_path,
                symbol_types,
            } => {
                let types = symbol_types.map(|ts| {
                    ts.iter()
                        .filter_map(|t| SymbolType::from_str(t).ok())
                        .collect()
                });

                let results = self
                    .service
                    .search_symbols_filtered(
                        &query,
                        file_path.as_deref(),
                        types,
                        50, // default limit
                    )
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Search failed: {}", e),
                    })?;

                let symbols = results
                    .into_iter()
                    .map(|r| LanguageSymbol {
                        id: r.symbol.id,
                        name: r.symbol.name,
                        symbol_type: r.symbol.symbol_type.to_string(),
                        file_path: r.symbol.file_path,
                        range: LanguageRange {
                            start: LanguagePosition {
                                line: r.symbol.range.start.line,
                                character: r.symbol.range.start.character,
                            },
                            end: LanguagePosition {
                                line: r.symbol.range.end.line,
                                character: r.symbol.range.end.character,
                            },
                        },
                        parent_id: None,
                        docstring: None,
                        signature: r.symbol.signature,
                    })
                    .collect();

                LanguageEvent::SymbolsFound { intent_id, symbols }
            }
            LanguageIntent::GetSymbolAt {
                file_path,
                line,
                character,
            } => {
                let symbol = self
                    .service
                    .get_symbol_at(&file_path, line, character)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Lookup failed: {}", e),
                    })?;

                let symbol_data = symbol.map(|s| LanguageSymbol {
                    id: s.id,
                    name: s.name,
                    symbol_type: s.symbol_type.to_string(),
                    file_path: s.file_path,
                    range: LanguageRange {
                        start: LanguagePosition {
                            line: s.range.start.line,
                            character: s.range.start.character,
                        },
                        end: LanguagePosition {
                            line: s.range.end.line,
                            character: s.range.end.character,
                        },
                    },
                    parent_id: None,
                    docstring: None,
                    signature: s.signature,
                });

                LanguageEvent::SymbolAt {
                    intent_id,
                    symbol: symbol_data,
                }
            }
            LanguageIntent::GetCompletions {
                file_path,
                line,
                character,
            } => {
                let items = self
                    .service
                    .get_completions(&file_path, line, character)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Completion failed: {}", e),
                    })?;

                let completion_items = items
                    .into_iter()
                    .map(|i| {
                        CompletionItem {
                            label: i.label,
                            kind: i.kind.map(|k| format!("{}", k)), // Map integer kind to string
                            detail: i.detail,
                            documentation: i.documentation.map(|d| match d {
                                serde_json::Value::String(s) => s,
                                serde_json::Value::Object(o) => o
                                    .get("value")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_default(),
                                _ => d.to_string(),
                            }),
                            insert_text: i.insert_text,
                        }
                    })
                    .collect();

                LanguageEvent::CompletionsReady {
                    intent_id,
                    items: completion_items,
                }
            }
            LanguageIntent::GetHover {
                file_path,
                line,
                character,
            } => {
                let hover = self
                    .service
                    .get_hover(&file_path, line, character)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Hover failed: {}", e),
                    })?;

                let (contents, range) = if let Some(h) = hover {
                    let content_str = self.extract_hover_content(&h.contents);
                    (Some(content_str), h.range.map(|r| self.map_range(r)))
                } else {
                    (None, None)
                };

                LanguageEvent::HoverReady {
                    intent_id,
                    contents,
                    range,
                }
            }
            LanguageIntent::GetDefinition {
                file_path,
                line,
                character,
            } => {
                let locations = self
                    .service
                    .get_definition(&file_path, line, character)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Definition failed: {}", e),
                    })?;

                let def_locations = locations
                    .into_iter()
                    .map(|l| self.map_location(l))
                    .collect();

                LanguageEvent::DefinitionReady {
                    intent_id,
                    locations: def_locations,
                }
            }
            LanguageIntent::GetReferences {
                file_path,
                line,
                character,
                include_declaration,
            } => {
                let locations = self
                    .service
                    .get_references(&file_path, line, character, include_declaration)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("References failed: {}", e),
                    })?;

                let ref_locations = locations
                    .into_iter()
                    .map(|l| self.map_location(l))
                    .collect();

                LanguageEvent::ReferencesReady {
                    intent_id,
                    locations: ref_locations,
                }
            }
            LanguageIntent::GetDocumentSymbols { file_path } => {
                let symbols = self
                    .service
                    .get_document_symbols_lsp(&file_path)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Document symbols failed: {}", e),
                    })?;

                let doc_symbols = symbols
                    .into_iter()
                    .map(|s| self.map_document_symbol(s))
                    .collect();

                LanguageEvent::DocumentSymbolsReady {
                    intent_id,
                    symbols: doc_symbols,
                }
            }
            LanguageIntent::GetDiagnostics { file_path } => {
                let diagnostics = self.service.get_diagnostics(&file_path);

                let diag_items = diagnostics
                    .into_iter()
                    .map(|d| LanguageDiagnostic {
                        range: self.map_range(d.range),
                        severity: d
                            .severity
                            .map(|s| format!("{:?}", s))
                            .unwrap_or_else(|| "information".to_string()),
                        code: d.code.map(|c| c.to_string()),
                        message: d.message,
                        source: d.source,
                    })
                    .collect();

                LanguageEvent::DiagnosticsUpdated {
                    file_path,
                    diagnostics: diag_items,
                }
            }

            // Document synchronization (no response events - just update state)
            LanguageIntent::DidOpen {
                file_path,
                content,
                language_id: _,
            } => {
                self.service
                    .did_open(&file_path, &content)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("DidOpen failed: {}", e),
                    })?;

                // Return a simple acknowledgment event
                LanguageEvent::FileIndexed {
                    file_path,
                    symbol_count: 0, // Placeholder, could be enhanced
                }
            }
            LanguageIntent::DidChange {
                file_path,
                content,
                version,
            } => {
                self.service
                    .did_change(&file_path, version as i32, &content)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("DidChange failed: {}", e),
                    })?;

                // No event for change - diagnostics will come separately
                return Ok(None);
            }
            LanguageIntent::DidClose { file_path } => {
                self.service
                    .did_close(&file_path)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("DidClose failed: {}", e),
                    })?;

                // No event for close
                return Ok(None);
            }

            LanguageIntent::GetSignatureHelp {
                file_path,
                line,
                character,
            } => {
                let help = self
                    .service
                    .get_signature_help(&file_path, line, character)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("SignatureHelp failed: {}", e),
                    })?;

                let (signatures, active_sig, active_param) = if let Some(h) = help {
                    let sigs: Vec<SignatureInfo> = h
                        .signatures
                        .into_iter()
                        .map(|s| SignatureInfo {
                            label: s.label,
                            documentation: s.documentation.and_then(|d| match d {
                                serde_json::Value::String(s) => Some(s),
                                serde_json::Value::Object(o) => o
                                    .get("value")
                                    .and_then(|v| v.as_str().map(|s| s.to_string())),
                                _ => None,
                            }),
                            parameters: s
                                .parameters
                                .into_iter()
                                .map(|p| ParameterInfo {
                                    label: match p.label {
                                        serde_json::Value::String(s) => s,
                                        serde_json::Value::Array(arr) if arr.len() == 2 => {
                                            // [start, end] offsets - just return as string for now
                                            format!("[{}, {}]", arr[0], arr[1])
                                        }
                                        _ => p.label.to_string(),
                                    },
                                    documentation: p.documentation.and_then(|d| match d {
                                        serde_json::Value::String(s) => Some(s),
                                        serde_json::Value::Object(o) => o
                                            .get("value")
                                            .and_then(|v| v.as_str().map(|s| s.to_string())),
                                        _ => None,
                                    }),
                                })
                                .collect(),
                        })
                        .collect();
                    (sigs, h.active_signature, h.active_parameter)
                } else {
                    (vec![], None, None)
                };

                LanguageEvent::SignatureHelpReady {
                    intent_id,
                    signatures,
                    active_signature: active_sig,
                    active_parameter: active_param,
                }
            }

            LanguageIntent::GetCodeActions {
                file_path,
                start_line,
                start_character,
                end_line,
                end_character,
            } => {
                let actions = self
                    .service
                    .get_code_actions(
                        &file_path,
                        start_line,
                        start_character,
                        end_line,
                        end_character,
                    )
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("CodeActions failed: {}", e),
                    })?;

                let action_items: Vec<CodeAction> = actions
                    .into_iter()
                    .map(|a| CodeAction {
                        title: a.title,
                        kind: a.kind,
                        diagnostics: if a.diagnostics.is_empty() {
                            None
                        } else {
                            Some(
                                a.diagnostics
                                    .into_iter()
                                    .map(|d| LanguageDiagnostic {
                                        range: self.map_range(d.range),
                                        severity: d
                                            .severity
                                            .map(|s| format!("{:?}", s))
                                            .unwrap_or_else(|| "information".to_string()),
                                        code: d.code.map(|c| c.to_string()),
                                        source: d.source,
                                        message: d.message,
                                    })
                                    .collect(),
                            )
                        },
                        is_preferred: a.is_preferred,
                    })
                    .collect();

                LanguageEvent::CodeActionsReady {
                    intent_id,
                    actions: action_items,
                }
            }
            LanguageIntent::Rename {
                file_path,
                line,
                character,
                new_name,
            } => {
                let edit = self
                    .service
                    .rename_symbol(&file_path, line, character, &new_name)
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Rename failed: {}", e),
                    })?;

                // Convert to Blade protocol types
                let blade_edit = edit.map(|e| {
                    let mut changes = std::collections::HashMap::new();
                    if let Some(e_changes) = e.changes {
                        for (uri, edits) in e_changes {
                            let converted_edits = edits
                                .into_iter()
                                .map(|te| LanguageTextEdit {
                                    range: self.map_range(te.range),
                                    new_text: te.new_text,
                                })
                                .collect();
                            // Simple URI to path conversion (may need better logic)
                            // Assuming file:// URIs for now or direct paths
                            let file_path = uri.replace("file://", "");
                            changes.insert(file_path, converted_edits);
                        }
                    }
                    LanguageWorkspaceEdit {
                        changes: Some(changes),
                    }
                });

                LanguageEvent::RenameEditsReady {
                    intent_id,
                    edit: blade_edit,
                }
            }
        };

        Ok(Some(BladeEventEnvelope {
            id: Uuid::new_v4(),
            causality_id: Some(intent_id.to_string()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            event: BladeEvent::Language(event_payload),
        }))
    }

    fn extract_hover_content(&self, content: &serde_json::Value) -> String {
        match content {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => arr
                .iter()
                .map(|v| self.extract_hover_content(v))
                .collect::<Vec<_>>()
                .join("\n\n"),
            serde_json::Value::Object(obj) => {
                if let Some(val) = obj.get("value").and_then(|v| v.as_str()) {
                    val.to_string()
                } else {
                    // Fallback to string representation
                    content.to_string()
                }
            }
            _ => content.to_string(),
        }
    }

    fn map_location(&self, loc: crate::lsp::types::Location) -> LanguageLocation {
        // file:// URI -> path
        let path = loc.uri.replace("file://", "");
        LanguageLocation {
            file_path: path,
            range: self.map_range(loc.range),
        }
    }

    fn map_range(&self, range: crate::lsp::types::Range) -> LanguageRange {
        LanguageRange {
            start: LanguagePosition {
                line: range.start.line,
                character: range.start.character,
            },
            end: LanguagePosition {
                line: range.end.line,
                character: range.end.character,
            },
        }
    }

    fn map_document_symbol(
        &self,
        sym: crate::lsp::types::DocumentSymbol,
    ) -> LanguageDocumentSymbol {
        LanguageDocumentSymbol {
            name: sym.name,
            kind: format!("{}", sym.kind), // i32 to string
            range: self.map_range(sym.range),
            selection_range: self.map_range(sym.selection_range),
            detail: sym.detail,
            children: sym
                .children
                .into_iter()
                .map(|child| self.map_document_symbol(child))
                .collect(),
        }
    }
}
