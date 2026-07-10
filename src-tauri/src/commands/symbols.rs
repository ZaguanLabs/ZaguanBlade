use crate::app_state::AppState;
use crate::language_service::{LanguageService, SymbolTraceDirection};
use crate::tree_sitter::{Symbol, SymbolRelationshipType};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct InspectorPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectorRange {
    pub start: InspectorPosition,
    pub end: InspectorPosition,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectorSymbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub symbol_type: String,
    pub file_path: String,
    pub range: InspectorRange,
    pub signature: Option<String>,
}

impl From<&Symbol> for InspectorSymbol {
    fn from(symbol: &Symbol) -> Self {
        Self {
            id: symbol.id.clone(),
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            symbol_type: symbol.symbol_type.to_string(),
            file_path: symbol.file_path.clone(),
            range: InspectorRange {
                start: InspectorPosition {
                    line: symbol.range.start.line,
                    character: symbol.range.start.character,
                },
                end: InspectorPosition {
                    line: symbol.range.end.line,
                    character: symbol.range.end.character,
                },
            },
            signature: symbol.signature.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectorGraphNode {
    pub symbol: InspectorSymbol,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectorGraphEdge {
    pub source_symbol_id: String,
    pub target_symbol_id: Option<String>,
    pub target_name: String,
    pub relationship_type: String,
    pub traversal_direction: String,
    pub line: u32,
    pub resolved: bool,
    pub observation_kind: String,
    pub resolution_strategy: Option<String>,
    pub effective_confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectorGraphResponse {
    pub root: InspectorSymbol,
    pub nodes: Vec<InspectorGraphNode>,
    pub edges: Vec<InspectorGraphEdge>,
    pub direction: String,
    pub relationship_types: Vec<String>,
    pub min_confidence: f32,
    pub truncated: bool,
    pub unresolved_edge_count: usize,
}

fn language_service(state: &AppState) -> Result<Arc<LanguageService>, String> {
    state
        .language_service
        .read()
        .map_err(|_| "Symbols Index lock is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "Symbols Index is not available for this workspace".to_string())
}

fn workspace_relative_file_path(state: &AppState, file_path: &str) -> Result<String, String> {
    let workspace_root = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .workspace
        .clone()
        .ok_or_else(|| "No workspace is open".to_string())?;
    let resolved = crate::commands::files::resolve_path_under_workspace_root(
        &workspace_root,
        Path::new(file_path),
    )?;
    let canonical_root = std::fs::canonicalize(&workspace_root).unwrap_or(workspace_root);
    resolved
        .strip_prefix(&canonical_root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "File is outside the current workspace".to_string())
}

fn parse_direction(direction: Option<&str>) -> Result<SymbolTraceDirection, String> {
    match direction
        .unwrap_or("both")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "incoming" | "inbound" | "in" => Ok(SymbolTraceDirection::Incoming),
        "outgoing" | "outbound" | "out" => Ok(SymbolTraceDirection::Outgoing),
        "both" => Ok(SymbolTraceDirection::Both),
        value => Err(format!(
            "Unknown graph direction `{value}`; expected incoming, outgoing, or both"
        )),
    }
}

fn default_relationship_types() -> Vec<SymbolRelationshipType> {
    vec![
        SymbolRelationshipType::Call,
        SymbolRelationshipType::Import,
        SymbolRelationshipType::Export,
        SymbolRelationshipType::Extends,
        SymbolRelationshipType::Implements,
        SymbolRelationshipType::Usage,
        SymbolRelationshipType::UsesType,
        SymbolRelationshipType::Handles,
    ]
}

fn parse_relationship_types(
    relationships: Option<Vec<String>>,
) -> Result<Vec<SymbolRelationshipType>, String> {
    let Some(relationships) = relationships else {
        return Ok(default_relationship_types());
    };
    if relationships.is_empty() {
        return Ok(default_relationship_types());
    }
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for relationship in relationships {
        let relationship_type = relationship.parse::<SymbolRelationshipType>()?;
        if seen.insert(relationship_type) {
            parsed.push(relationship_type);
        }
    }
    Ok(parsed)
}

#[tauri::command]
pub async fn resolve_local_symbol_at(
    file_path: String,
    line: u32,
    character: u32,
    state: State<'_, AppState>,
) -> Result<Option<InspectorSymbol>, String> {
    let file_path = workspace_relative_file_path(&state, &file_path)?;
    let service = language_service(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let symbol = service
            .get_symbol_at(&file_path, line, character)
            .map_err(|error| error.to_string())?;
        let symbol = match symbol {
            Some(symbol) => Some(symbol),
            None => service
                .get_file_module_symbol(&file_path)
                .map_err(|error| error.to_string())?,
        };
        Ok(symbol.as_ref().map(InspectorSymbol::from))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_local_symbol_graph(
    symbol_id: String,
    direction: Option<String>,
    relationships: Option<Vec<String>>,
    min_confidence: Option<f32>,
    state: State<'_, AppState>,
) -> Result<InspectorGraphResponse, String> {
    let service = language_service(&state)?;
    let direction = parse_direction(direction.as_deref())?;
    let relationship_types = parse_relationship_types(relationships)?;
    let min_confidence = min_confidence.unwrap_or(0.5).clamp(0.0, 1.0);
    tauri::async_runtime::spawn_blocking(move || {
        let initial = service
            .get_symbol(&symbol_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "The selected symbol is no longer indexed".to_string())?;
        service
            .get_file_symbols(&initial.file_path)
            .map_err(|error| error.to_string())?;
        let seed = service
            .get_symbol(&symbol_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                "The selected symbol changed while its file was refreshed".to_string()
            })?;
        let trace = service
            .trace_symbol_graph(&seed, &relationship_types, direction, 1, 200, 50)
            .map_err(|error| error.to_string())?;
        let mut included_symbol_ids = HashSet::from([seed.id.clone()]);
        let mut unresolved_edge_count = 0usize;
        let edges = trace
            .edges
            .into_iter()
            .filter_map(|edge| {
                let effective_confidence = edge
                    .resolution_confidence
                    .unwrap_or(if edge.resolved { 0.75 } else { 0.0 })
                    .clamp(0.0, 1.0);
                if effective_confidence < min_confidence {
                    return None;
                }
                if !edge.resolved {
                    unresolved_edge_count += 1;
                }
                included_symbol_ids.insert(edge.source_symbol.id.clone());
                if let Some(target) = edge.target_symbol.as_ref() {
                    included_symbol_ids.insert(target.id.clone());
                }
                Some(InspectorGraphEdge {
                    source_symbol_id: edge.source_symbol.id,
                    target_symbol_id: edge.target_symbol.as_ref().map(|symbol| symbol.id.clone()),
                    target_name: edge.target_name,
                    relationship_type: edge.relationship_type.to_string(),
                    traversal_direction: edge.direction.as_str().to_string(),
                    line: edge.line,
                    resolved: edge.resolved,
                    observation_kind: edge.observation_kind.as_str().to_string(),
                    resolution_strategy: edge.resolution_strategy,
                    effective_confidence,
                })
            })
            .collect::<Vec<_>>();
        let nodes = trace
            .nodes
            .into_iter()
            .filter(|node| included_symbol_ids.contains(&node.symbol.id))
            .map(|node| InspectorGraphNode {
                symbol: InspectorSymbol::from(&node.symbol),
                depth: node.depth,
            })
            .collect::<Vec<_>>();

        Ok(InspectorGraphResponse {
            root: InspectorSymbol::from(&seed),
            nodes,
            edges,
            direction: direction.as_str().to_string(),
            relationship_types: relationship_types.iter().map(ToString::to_string).collect(),
            min_confidence,
            truncated: trace.truncated,
            unresolved_edge_count,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{parse_direction, parse_relationship_types};
    use crate::language_service::SymbolTraceDirection;
    use crate::tree_sitter::SymbolRelationshipType;

    #[test]
    fn inspector_filters_parse_deterministically() {
        assert!(matches!(
            parse_direction(Some("incoming")).unwrap(),
            SymbolTraceDirection::Incoming
        ));
        let relationships = parse_relationship_types(Some(vec![
            "call".to_string(),
            "call".to_string(),
            "uses_type".to_string(),
        ]))
        .unwrap();
        assert_eq!(
            relationships,
            vec![
                SymbolRelationshipType::Call,
                SymbolRelationshipType::UsesType
            ]
        );
    }
}
