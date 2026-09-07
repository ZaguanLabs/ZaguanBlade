//! Immutable MCP discovery snapshots. A catalog records descriptions, not consent,
//! connection liveness, or permission to execute. Never dispatch by original name.
use super::{store::fingerprint, RuntimeError};
use rmcp::model::Tool;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub const MAX_TOOLS: usize = 512;
const MAX_TOOL_BYTES: usize = 128 * 1024;
const MAX_CATALOG_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 32;

#[derive(Debug, Serialize)]
pub struct CatalogTool {
    pub alias: String,
    /// Preserve schemas and SDK-supported metadata without provider adaptation.
    pub definition: Tool,
}

#[derive(Debug, Serialize)]
pub struct McpCatalog {
    schema_version: u32,
    integration_id: Uuid,
    revision: String,
    tools: Vec<CatalogTool>,
}

impl McpCatalog {
    pub fn tools(&self) -> &[CatalogTool] {
        &self.tools
    }

    /// Resolves only this immutable observation. A future dispatcher must ALSO
    /// check current workspace/configuration, connection and invocation approval.
    pub fn resolve(&self, alias: &str, revision: &str) -> Result<&CatalogTool, RuntimeError> {
        if self.revision != revision {
            return Err(RuntimeError::CatalogChanged);
        }
        self.tools
            .iter()
            .find(|tool| tool.alias == alias)
            .ok_or(RuntimeError::InvalidCatalog)
    }
}

pub struct CatalogBuilder {
    integration_id: Uuid,
    tools: BTreeMap<String, CatalogTool>,
    aliases: HashSet<String>,
    bytes: usize,
}

impl CatalogBuilder {
    pub fn new(integration_id: Uuid) -> Self {
        Self {
            integration_id,
            tools: BTreeMap::new(),
            aliases: HashSet::new(),
            bytes: 0,
        }
    }

    pub fn push(&mut self, definition: Tool) -> Result<(), RuntimeError> {
        let name = definition.name.as_ref();
        if name.is_empty()
            || name.chars().count() > 128
            || name.chars().any(char::is_control)
            || self.tools.contains_key(name)
        {
            return Err(RuntimeError::InvalidCatalog);
        }
        if self.tools.len() >= MAX_TOOLS {
            return Err(RuntimeError::OutputLimit);
        }
        // Full integration UUID plus a name digest fits a 64-character function
        // name. Detect a truncated digest collision instead of ever misrouting it.
        let digest = fingerprint(name.as_bytes());
        let alias = format!("mcp_{}_{}", self.integration_id.simple(), &digest[..24]);
        if self.aliases.contains(&alias) {
            return Err(RuntimeError::InvalidCatalog);
        }
        let tool = CatalogTool { alias, definition };
        let value = serde_json::to_value(&tool).map_err(|_| RuntimeError::InvalidCatalog)?;
        check_depth(&value, 0)?;
        let size = serde_json::to_vec(&value)
            .map_err(|_| RuntimeError::InvalidCatalog)?
            .len();
        if size > MAX_TOOL_BYTES || size > MAX_CATALOG_BYTES.saturating_sub(self.bytes) {
            return Err(RuntimeError::OutputLimit);
        }
        self.bytes += size;
        self.aliases.insert(tool.alias.clone());
        self.tools.insert(tool.definition.name.to_string(), tool);
        Ok(())
    }

    pub fn finish(self) -> Result<McpCatalog, RuntimeError> {
        let mut catalog = McpCatalog {
            schema_version: 1,
            integration_id: self.integration_id,
            revision: String::new(),
            tools: self.tools.into_values().collect(),
        };
        // Canonicalize object keys as well as tool order. This remains stable if
        // serde_json's preserve_order feature is enabled by a future dependency.
        let value = serde_json::to_value(&catalog).map_err(|_| RuntimeError::InvalidCatalog)?;
        let bytes =
            serde_json::to_vec(&canonical(value)).map_err(|_| RuntimeError::InvalidCatalog)?;
        // Account for the envelope, punctuation and final SHA-256 revision too.
        if bytes.len().saturating_add(64) > MAX_CATALOG_BYTES {
            return Err(RuntimeError::OutputLimit);
        }
        catalog.revision = fingerprint(&bytes);
        Ok(catalog)
    }
}

fn check_depth(value: &Value, depth: usize) -> Result<(), RuntimeError> {
    if depth > MAX_DEPTH {
        return Err(RuntimeError::OutputLimit);
    }
    match value {
        Value::Object(map) => {
            for child in map.values() {
                check_depth(child, depth + 1)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                check_depth(child, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str) -> Tool {
        serde_json::from_value(json!({"name":name,"description":"From the server",
            "inputSchema":{"type":"object","properties":{"query":{"$ref":"#/$defs/query"}},
                "$defs":{"query":{"type":"string","minLength":3}},"required":["query"],"additionalProperties":false},
            "outputSchema":{"type":"object","properties":{"count":{"type":"integer"}}},
            "annotations":{"readOnlyHint":true},"_meta":{"example":"preserved"},
            "icons":[{"src":"https://invalid.example/icon.svg"}]})).unwrap()
    }
    fn catalog(id: Uuid, tools: Vec<Tool>) -> McpCatalog {
        let mut builder = CatalogBuilder::new(id);
        for tool in tools {
            builder.push(tool).unwrap();
        }
        builder.finish().unwrap()
    }

    #[test]
    fn same_names_from_different_servers_have_distinct_routes() {
        let a = catalog(Uuid::new_v4(), vec![tool("read_file_range")]);
        let b = catalog(Uuid::new_v4(), vec![tool("read_file_range")]);
        let alias = &a.tools[0].alias;
        assert_ne!(alias, &b.tools[0].alias);
        assert!(alias.len() <= 64);
        assert!(alias
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_'));
        assert_eq!(
            a.resolve(alias, &a.revision).unwrap().definition.name,
            "read_file_range"
        );
        assert!(b.resolve(alias, &b.revision).is_err());
        assert!(a.resolve("read_file_range", &a.revision).is_err());
    }

    #[test]
    fn metadata_changes_retire_revisions_without_remapping_aliases() {
        let id = Uuid::new_v4();
        let original = catalog(id, vec![tool("search")]);
        let mut changed = tool("search");
        changed.description = Some("Changed behavior".into());
        let next = catalog(id, vec![changed]);
        assert_eq!(original.tools[0].alias, next.tools[0].alias);
        assert_ne!(original.revision, next.revision);
        assert_eq!(
            next.resolve(&original.tools[0].alias, &original.revision)
                .unwrap_err(),
            RuntimeError::CatalogChanged
        );
    }

    #[test]
    fn discovery_order_and_object_key_order_do_not_change_revision() {
        let id = Uuid::new_v4();
        let a = catalog(id, vec![tool("search"), tool("Search")]);
        let b = catalog(id, vec![tool("Search"), tool("search")]);
        assert_eq!(a.revision, b.revision);
        assert_eq!(
            canonical(json!({"b":{"d":1,"c":2},"a":[]})),
            json!({"a":[],"b":{"c":2,"d":1}})
        );
        assert_ne!(a.tools[0].alias, a.tools[1].alias);
    }

    #[test]
    fn schemas_and_untrusted_metadata_are_preserved_without_fetching_links() {
        let definition = tool("namespace.search");
        let expected = serde_json::to_value(&definition).unwrap();
        let catalog = catalog(Uuid::new_v4(), vec![definition]);
        assert_eq!(
            serde_json::to_value(&catalog.tools[0].definition).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(&catalog).unwrap()["schema_version"], 1);
    }

    #[test]
    fn duplicate_names_and_invalid_names_are_rejected() {
        let mut builder = CatalogBuilder::new(Uuid::new_v4());
        builder.push(tool("search")).unwrap();
        assert_eq!(
            builder.push(tool("search")),
            Err(RuntimeError::InvalidCatalog)
        );
        for name in ["".into(), "line\nbreak".into(), "x".repeat(129)] {
            assert_eq!(builder.push(tool(&name)), Err(RuntimeError::InvalidCatalog));
        }
    }

    #[test]
    fn tool_count_bytes_and_depth_are_bounded() {
        let mut builder = CatalogBuilder::new(Uuid::new_v4());
        for i in 0..MAX_TOOLS {
            builder.push(tool(&format!("t{i}"))).unwrap();
        }
        assert_eq!(builder.push(tool("excess")), Err(RuntimeError::OutputLimit));
        let mut builder = CatalogBuilder::new(Uuid::new_v4());
        let mut large = tool("large");
        large.description = Some("x".repeat(MAX_TOOL_BYTES).into());
        assert_eq!(builder.push(large), Err(RuntimeError::OutputLimit));
        let mut value = json!({});
        for _ in 0..MAX_DEPTH {
            value = json!({"nested":value});
        }
        let mut deep = tool("deep");
        deep.input_schema = std::sync::Arc::new(value.as_object().unwrap().clone());
        assert_eq!(builder.push(deep), Err(RuntimeError::OutputLimit));
        for i in 0..16 {
            let mut item = tool(&format!("large{i}"));
            item.description = Some("x".repeat(100_000).into());
            let result = builder.push(item);
            if result == Err(RuntimeError::OutputLimit) {
                assert!(i < 11);
                return;
            }
            result.unwrap();
        }
        panic!("catalog budget was not enforced");
    }
}
