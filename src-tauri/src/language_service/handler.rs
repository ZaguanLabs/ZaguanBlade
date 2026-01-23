//! Blade Protocol Handler for Language Service
//!
//! Handles `LanguageIntent`s and dispatches them to the `LanguageService`
//! or `SymbolStore`, returning appropriate `LanguageEvent`s.

use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::blade_protocol::{
    BladeError, BladeEvent, BladeEventEnvelope, BladeResult, LanguageEvent, LanguageIntent,
    LanguagePosition, LanguageRange, LanguageSymbol,
};
use crate::language_service::LanguageService;
use crate::tree_sitter::SymbolType;
use tauri::async_runtime::spawn_blocking;

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
        let service = self.service.clone();
        let event_payload = match intent {
            LanguageIntent::IndexFile { file_path } => {
                let s = service.clone();
                let f = file_path.clone();
                let symbols = spawn_blocking(move || s.index_file(&f))
                    .await
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Task join error: {}", e),
                    })?
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
                let s = service.clone();
                let stats = spawn_blocking(move || s.index_directory("."))
                    .await
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Task join error: {}", e),
                    })?
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
                let s = service.clone();
                let types = symbol_types.map(|ts| {
                    ts.iter()
                        .filter_map(|t| SymbolType::from_str(t).ok())
                        .collect()
                });

                let results = spawn_blocking(move || {
                    s.search_symbols_filtered(
                        &query,
                        file_path.as_deref(),
                        types,
                        50, // default limit
                    )
                })
                .await
                .map_err(|e| BladeError::Internal {
                    trace_id: Uuid::new_v4().to_string(),
                    message: format!("Task join error: {}", e),
                })?
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
                let s = service.clone();
                let symbol = spawn_blocking(move || s.get_symbol_at(&file_path, line, character))
                    .await
                    .map_err(|e| BladeError::Internal {
                        trace_id: Uuid::new_v4().to_string(),
                        message: format!("Task join error: {}", e),
                    })?
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
            LanguageIntent::ZlpMessage { .. } => {
                return Err(BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: "ZlpMessage should be handled by protocol dispatcher".to_string(),
                });
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
}
