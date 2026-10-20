//! Immutable native-provider catalog. Preserve JSON Schema constraints verbatim;
//! reject unsupported schema dialects/references instead of weakening them.
use super::{catalog::McpCatalog, permissions::CallTarget, store::fingerprint};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const MAX_NATIVE_TOOLS: usize = 32;
const MAX_SCHEMA_BYTES: usize = 128 * 1024;
#[derive(Clone)]
pub struct NativeTool {
    pub target: CallTarget,
    pub integration_id: Uuid,
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
    pub fn add(
        &mut self,
        connection: Uuid,
        integration: Uuid,
        server_name: &str,
        catalog: &McpCatalog,
    ) {
        for tool in catalog.tools() {
            let name = self.function_name(server_name, &tool.definition.name, &tool.alias);
            let parameters = Value::Object((*tool.definition.input_schema).clone());
            let schema = json!({"type":"function","function":{
                "name":name, "description":format!("MCP: {} / {}. {}", server_name, tool.definition.name, tool.definition.description.as_deref().unwrap_or("")),
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
                name,
                NativeTool {
                    target: CallTarget {
                        connection_id: connection,
                        alias: tool.alias.clone(),
                        catalog_revision: catalog.revision().into(),
                    },
                    integration_id: integration,
                    server_name: server_name.into(),
                    original_name: tool.definition.name.to_string(),
                    schema,
                },
            );
        }
    }
}
impl NativeCatalog {
    /// `tool_catalog.external` entries for a cloud request, keyed by the same
    /// names the model calls, and the tools left out. zcoderd refuses a whole
    /// request over one entry it cannot accept, so a schema with a `$ref` key
    /// holding anything but a local reference (zcoderd checks every key, data
    /// included) stays local, as does anything past the 128 KiB budget.
    pub fn cloud_entries(&self) -> (Vec<Value>, Vec<(String, ExcludedTool)>) {
        let (mut entries, mut excluded, mut bytes) = (Vec::new(), Vec::new(), 2);
        for (name, tool) in &self.tools {
            let parameters = &tool.schema["function"]["parameters"];
            let entry = json!({"alias":name,
                "origin":{"kind":"mcp","integration_id":tool.integration_id,"server_name":tool.server_name},
                "tool_name":tool.original_name,"description":tool.schema["function"]["description"],
                "input_schema":parameters});
            let size = serde_json::to_vec(&entry)
                .map(|v| v.len() + 1)
                .unwrap_or(usize::MAX);
            let reason = if !only_local_refs(parameters) {
                Some("schema")
            } else if size > MAX_SCHEMA_BYTES.saturating_sub(bytes) {
                Some("budget")
            } else {
                None
            };
            match reason {
                Some(reason) => excluded.push((
                    name.clone(),
                    ExcludedTool {
                        server_name: tool.server_name.clone(),
                        tool_name: tool.original_name.clone(),
                        reason,
                    },
                )),
                None => {
                    bytes += size;
                    entries.push(entry);
                }
            }
        }
        (entries, excluded)
    }
}
/// zcoderd's rule: every `$ref` key, wherever it appears, holds a local reference.
fn only_local_refs(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().all(|(key, child)| {
            (key != "$ref" || child.as_str().is_some_and(|r| r.starts_with('#')))
                && only_local_refs(child)
        }),
        Value::Array(items) => items.iter().all(only_local_refs),
        _ => true,
    }
}
impl NativeCatalog {
    /// The name the model calls, such as `mcp__bit_lantern__symbol_search`.
    /// Models pick tools by name, so it says what the tool is. The opaque
    /// catalog alias stays the call target. A digest of that alias settles
    /// collisions and over-long names, so a name never routes to another tool.
    fn function_name(&self, server: &str, tool: &str, alias: &str) -> String {
        let readable = format!(
            "mcp__{}__{}",
            slug(server, "server").to_ascii_lowercase(),
            slug(tool, "tool")
        );
        if readable.len() <= MAX_FUNCTION_NAME && !self.tools.contains_key(&readable) {
            return readable;
        }
        let digest = fingerprint(alias.as_bytes());
        let mut name = readable;
        name.truncate(MAX_FUNCTION_NAME - 9);
        format!("{name}_{}", &digest[..8])
    }
}
const MAX_FUNCTION_NAME: usize = 64;
/// Letters, digits, `-` and single underscores: what every provider accepts.
fn slug(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_');
    if out.is_empty() {
        fallback.into()
    } else {
        out.into()
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
        output.add(Uuid::new_v4(), Uuid::new_v4(), "Fixture", &input);
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
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "First",
            &catalog(Uuid::new_v4(), 1),
        );
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Second",
            &catalog(Uuid::new_v4(), 1),
        );
        assert_eq!(output.tools.len(), 2);
        assert!(output.tools.contains_key("mcp__first__tool_00"));
        assert!(output.tools.contains_key("mcp__second__tool_00"));
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
    #[test]
    fn cloud_entries_carry_origin_and_leave_out_what_zcoderd_would_refuse() {
        let mut output = NativeCatalog::default();
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Bit Lantern",
            &catalog(Uuid::new_v4(), 1),
        );
        let docs = Uuid::new_v4();
        let mut builder = CatalogBuilder::new(Uuid::new_v4());
        builder.push(serde_json::from_value(json!({"name":"search","description":"Find code",
            "inputSchema":{"type":"object","properties":{"q":{"type":"string","maxLength":9007199254740993u64}},"$defs":{"a":{"type":"string"}},"items":{"$ref":"#/$defs/a"}}})).unwrap()).unwrap();
        output.add(Uuid::new_v4(), docs, "Docs", &builder.finish().unwrap());
        // Blade accepts a property named `$ref`; zcoderd does not.
        assert_eq!(output.tools.len(), 2);
        let (entries, excluded) = output.cloud_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["alias"], "mcp__docs__search");
        assert_eq!(entries[0]["origin"]["kind"], "mcp");
        assert_eq!(entries[0]["origin"]["integration_id"], docs.to_string());
        assert_eq!(entries[0]["origin"]["server_name"], "Docs");
        assert_eq!(entries[0]["tool_name"], "search");
        assert_eq!(
            entries[0]["input_schema"]["properties"]["q"]["maxLength"],
            9007199254740993u64
        );
        assert!(entries[0]["description"]
            .as_str()
            .unwrap()
            .contains("Find code"));
        assert_eq!(excluded.len(), 1);
        assert_eq!(excluded[0].0, "mcp__bit_lantern__tool_00");
        assert_eq!(excluded[0].1.reason, "schema");
        assert!(only_local_refs(&json!({"items":[{"$ref":"#/a"}]})));
        assert!(!only_local_refs(&json!({"default":{"$ref":"literal"}})));
    }
    #[test]
    fn model_names_are_readable_valid_and_never_shared() {
        let mut output = NativeCatalog::default();
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Bit Lantern",
            &catalog(Uuid::new_v4(), 1),
        );
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "bit-lantern!",
            &catalog(Uuid::new_v4(), 1),
        );
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Bit Lantern",
            &catalog(Uuid::new_v4(), 1),
        );
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &"ñ".repeat(80),
            &catalog(Uuid::new_v4(), 1),
        );
        output.add(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &"long".repeat(40),
            &catalog(Uuid::new_v4(), 1),
        );
        assert_eq!(output.tools.len(), 5);
        assert!(output.tools.contains_key("mcp__bit_lantern__tool_00"));
        assert!(output.tools.contains_key("mcp__bit-lantern__tool_00"));
        assert!(output.tools.contains_key("mcp__server__tool_00"));
        for (name, tool) in &output.tools {
            assert!(name.starts_with("mcp_"));
            assert!(name.len() <= MAX_FUNCTION_NAME);
            assert!(name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
            assert_eq!(tool.schema["function"]["name"], name.as_str());
        }
        let targets: std::collections::HashSet<_> = output
            .tools
            .values()
            .map(|tool| tool.target.connection_id)
            .collect();
        assert_eq!(targets.len(), 5);
    }
}
