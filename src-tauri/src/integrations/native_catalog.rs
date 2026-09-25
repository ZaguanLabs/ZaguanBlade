//! Immutable native-provider catalog. Preserve JSON Schema constraints verbatim;
//! reject unsupported schema dialects/references instead of weakening them.
use super::{catalog::McpCatalog, permissions::CallTarget};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const MAX_NATIVE_TOOLS: usize = 32;
const MAX_SCHEMA_BYTES: usize = 128 * 1024;
#[derive(Clone)]
pub struct NativeTool {
    pub target: CallTarget,
    pub server_name: String,
    pub original_name: String,
    pub schema: Value,
}
#[derive(Clone, Serialize)]
pub struct ExcludedTool {
    pub server_name: String,
    pub tool_name: String,
    pub reason: &'static str,
}
#[derive(Default)]
pub struct NativeCatalog {
    pub tools: BTreeMap<String, NativeTool>,
    pub excluded: Vec<ExcludedTool>,
    bytes: usize,
}
impl NativeCatalog {
    pub fn add(&mut self, connection: Uuid, server_name: &str, catalog: &McpCatalog) {
        for tool in catalog.tools() {
            let parameters = Value::Object((*tool.definition.input_schema).clone());
            let schema = json!({"type":"function","function":{
                "name":tool.alias, "description":format!("MCP: {} / {}. {}", server_name, tool.definition.name, tool.definition.description.as_deref().unwrap_or("")),
                "parameters":parameters}});
            let bytes = serde_json::to_vec(&schema)
                .map(|v| v.len())
                .unwrap_or(usize::MAX);
            let reason = if !compatible_schema(&parameters) {
                Some("schema")
            } else if self.tools.len() >= MAX_NATIVE_TOOLS
                || bytes > MAX_SCHEMA_BYTES.saturating_sub(self.bytes)
            {
                Some("budget")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.excluded.push(ExcludedTool {
                    server_name: server_name.into(),
                    tool_name: tool.definition.name.to_string(),
                    reason,
                });
                continue;
            }
            self.bytes += bytes;
            self.tools.insert(
                tool.alias.clone(),
                NativeTool {
                    target: CallTarget {
                        connection_id: connection,
                        alias: tool.alias.clone(),
                        catalog_revision: catalog.revision().into(),
                    },
                    server_name: server_name.into(),
                    original_name: tool.definition.name.to_string(),
                    schema,
                },
            );
        }
    }
}
fn compatible_schema(value: &Value) -> bool {
    if value.get("type").and_then(Value::as_str) != Some("object") {
        return false;
    }
    fn visit(value: &Value) -> bool {
        let Some(map) = value.as_object() else {
            return value.is_boolean();
        };
        map.iter().all(|(key, value)| match key.as_str() {
            "$ref" => value.as_str().is_some_and(|v| v.starts_with('#')),
            "$schema" => value.as_str().is_some_and(|v| {
                matches!(
                    v,
                    "https://json-schema.org/draft/2020-12/schema"
                        | "http://json-schema.org/draft-07/schema#"
                        | "https://json-schema.org/draft-07/schema"
                )
            }),
            "$dynamicRef" | "$recursiveRef" | "$id" => false,
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => {
                value
                    .as_object()
                    .is_some_and(|items| items.values().all(visit))
            }
            "dependencies" => value.as_object().is_some_and(|items| {
                items.values().all(|item| {
                    item.as_array()
                        .map(|names| names.iter().all(Value::is_string))
                        .unwrap_or_else(|| visit(item))
                })
            }),
            "contentSchema" => visit(value),
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => value
                .as_array()
                .is_some_and(|items| items.iter().all(visit)),
            "items" if value.is_array() => value
                .as_array()
                .is_some_and(|items| items.iter().all(visit)),
            "not"
            | "if"
            | "then"
            | "else"
            | "items"
            | "additionalItems"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "contains"
            | "propertyNames" => visit(value),
            // Enum/default/examples and property names are data, not schemas.
            _ => true,
        })
    }
    visit(value)
}

#[cfg(test)]
mod tests {
    use super::super::catalog::CatalogBuilder;
    use super::*;
    fn catalog(id: Uuid, count: usize) -> McpCatalog {
        let mut builder = CatalogBuilder::new(id);
        for i in 0..count {
            builder.push(serde_json::from_value(json!({"name":format!("tool_{i:02}"),"inputSchema":{"type":"object","properties":{"$ref":{"type":"string","enum":["external"]}},"additionalProperties":false}})).unwrap()).unwrap();
        }
        builder.finish().unwrap()
    }
    #[test]
    fn bounded_catalog_preserves_constraints_and_reports_omissions() {
        let input = catalog(Uuid::new_v4(), 35);
        let mut output = NativeCatalog::default();
        output.add(Uuid::new_v4(), "Fixture", &input);
        assert_eq!(output.tools.len(), 32);
        assert_eq!(output.excluded.len(), 3);
        for tool in output.tools.values() {
            assert_eq!(
                tool.schema["function"]["parameters"]["additionalProperties"],
                false
            );
            assert_eq!(tool.target.catalog_revision, input.revision());
        }
    }
    #[test]
    fn names_are_isolated_and_external_refs_are_rejected_without_mutation() {
        let mut output = NativeCatalog::default();
        output.add(Uuid::new_v4(), "First", &catalog(Uuid::new_v4(), 1));
        output.add(Uuid::new_v4(), "Second", &catalog(Uuid::new_v4(), 1));
        assert_eq!(output.tools.len(), 2);
        assert!(!compatible_schema(
            &json!({"type":"object","properties":{"query":{"$ref":"https://external/schema"}}})
        ));
        assert!(compatible_schema(
            &json!({"type":"object","properties":{"$ref":{"type":"string"}},"default":{"$ref":"literal"}})
        ));
        assert!(!compatible_schema(
            &json!({"type":"object","dependencies":{"query":{"$ref":"https://external/schema"}}})
        ));
        assert!(compatible_schema(
            &json!({"type":"object","dependencies":{"query":["other"]}})
        ));
    }
}
