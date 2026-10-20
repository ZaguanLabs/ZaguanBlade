//! The tool catalog Blade sends with a cloud chat request once zcoderd has
//! negotiated `client_tool_catalog_v1`. zcoderd offers the intersection of its
//! mirrored schemas and `builtin.names`, keeps its own server tools, and drops
//! index tools and index guidance when `symbols_index_enabled` is false.
//! With `client_external_tools_v1` it also offers the turn's MCP tools under
//! the names the model calls; their calls come back to Blade to run.

use super::native_turn::NativeMcpTurn;
use crate::blade_ws_client::OmittedTool;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Negotiated in `authenticate` and listed in `authenticated.capabilities`.
pub const CAPABILITY: &str = "client_tool_catalog_v1";
/// Lets the catalog carry `external` (MCP) tools.
pub const EXTERNAL_CAPABILITY: &str = "client_external_tools_v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudToolCatalog {
    schema_version: u32,
    revision: String,
    builtin: BuiltinCatalog,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    external: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct BuiltinCatalog {
    mode: &'static str,
    names: Vec<&'static str>,
    symbols_index_enabled: bool,
}

impl CloudToolCatalog {
    /// Every built-in Blade runs itself, minus index tools when the index is
    /// off, and the given `external` entries (`NativeMcpTurn::cloud_tools`).
    pub fn new(symbols_index_enabled: bool, external: Vec<serde_json::Value>) -> Self {
        let mut names: Vec<&'static str> = crate::tools::CLIENT_TOOL_NAMES
            .iter()
            .copied()
            .filter(|name| symbols_index_enabled || !crate::index_policy::requires_index(name))
            .collect();
        names.sort_unstable();
        names.dedup();
        let mut catalog = Self {
            schema_version: 1,
            revision: String::new(),
            builtin: BuiltinCatalog {
                mode: "allow_list",
                names,
                symbols_index_enabled,
            },
            external,
        };
        // The revision covers everything else, so equal catalogs share one.
        // zcoderd expects bare hex, without the `sha256:` prefix Blade uses elsewhere.
        let digest = Sha256::digest(serde_json::to_vec(&catalog).expect("catalog serializes"));
        catalog.revision = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        catalog
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn has_external(&self) -> bool {
        !self.external.is_empty()
    }
}

/// What one cloud chat request sends for tools, decided from zcoderd's
/// `authenticated.capabilities`.
pub(crate) struct CloudRequest {
    pub legacy_tools: Option<Vec<Value>>,
    pub catalog: Option<CloudToolCatalog>,
}

impl CloudRequest {
    /// With the negotiated catalog zcoderd follows the index setting itself;
    /// otherwise the legacy `tools` field carries it. MCP tools need the
    /// catalog and the external-tools capability; without them the turn's MCP
    /// tools are switched off.
    pub(crate) fn new(
        capabilities: &[String],
        index_enabled: bool,
        turn: Option<&NativeMcpTurn>,
        legacy_tools: impl FnOnce() -> Vec<Value>,
    ) -> Self {
        let supports = |keyword: &str| capabilities.iter().any(|c| c == keyword);
        let catalog_supported = supports(CAPABILITY);
        let external = match turn {
            Some(turn) if catalog_supported && supports(EXTERNAL_CAPABILITY) => turn.cloud_tools(),
            Some(turn) => {
                turn.cloud_unsupported();
                Vec::new()
            }
            None => Vec::new(),
        };
        let catalog = catalog_supported.then(|| CloudToolCatalog::new(index_enabled, external));
        Self {
            legacy_tools: (catalog.is_none() && !index_enabled).then(legacy_tools),
            catalog,
        }
    }

    /// Applies zcoderd's answer from `session_created` to the turn whose MCP
    /// tools were sent. False when a catalog was sent and its revision was not
    /// confirmed.
    pub(crate) fn acknowledge(
        &self,
        turn: Option<&NativeMcpTurn>,
        applied: Option<&str>,
        omitted: &[OmittedTool],
    ) -> bool {
        let Some(catalog) = &self.catalog else {
            return true;
        };
        let confirmed = applied == Some(catalog.revision());
        if let (true, Some(turn)) = (catalog.has_external(), turn) {
            turn.cloud_acknowledged(confirmed, omitted);
        }
        confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keywords(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn each_server_kind_gets_the_tools_field_it_understands() {
        let legacy = || vec![serde_json::json!({"name":"read_file"})];
        let both = keywords(&[CAPABILITY, EXTERNAL_CAPABILITY]);

        let new = CloudRequest::new(&both, false, None, legacy);
        assert!(new.legacy_tools.is_none());
        let catalog = new.catalog.as_ref().unwrap();
        assert!(!catalog.builtin.symbols_index_enabled && !catalog.has_external());
        assert!(new.acknowledge(None, Some(catalog.revision()), &[]));
        assert!(!new.acknowledge(None, Some("other"), &[]));
        assert!(!new.acknowledge(None, None, &[]));

        let part_a = CloudRequest::new(&keywords(&[CAPABILITY]), true, None, legacy);
        assert!(part_a.catalog.is_some() && part_a.legacy_tools.is_none());

        let old_index_off = CloudRequest::new(&keywords(&["host_skills_v1"]), false, None, legacy);
        assert!(old_index_off.catalog.is_none());
        assert_eq!(old_index_off.legacy_tools.unwrap().len(), 1);
        let old_index_on = CloudRequest::new(&[], true, None, || panic!("not needed"));
        assert!(old_index_on.catalog.is_none() && old_index_on.legacy_tools.is_none());
        assert!(old_index_on.acknowledge(None, None, &[]));
        // The external keyword alone means nothing without the catalog.
        let external_only =
            CloudRequest::new(&keywords(&[EXTERNAL_CAPABILITY]), true, None, legacy);
        assert!(external_only.catalog.is_none());
    }

    #[test]
    fn the_cloud_catalog_lists_only_what_blade_runs_and_follows_the_index() {
        let on = CloudToolCatalog::new(true, Vec::new());
        let off = CloudToolCatalog::new(false, Vec::new());
        let json = serde_json::to_value(&off).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["builtin"]["mode"], "allow_list");
        assert_eq!(json["builtin"]["symbols_index_enabled"], false);
        let revision = json["revision"].as_str().unwrap();
        assert_eq!(revision.len(), 64);
        assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(json.get("external").is_none());

        assert!(on.builtin.names.contains(&"symbol_search"));
        assert!(on.builtin.names.contains(&"run_command"));
        assert!(off.builtin.names.contains(&"read_file_range"));
        assert!(off
            .builtin
            .names
            .iter()
            .all(|name| !crate::index_policy::requires_index(name)));
        for server_tool in [
            "todo_write",
            "attempt_completion",
            "generate_image",
            "new_task",
        ] {
            assert!(!on.builtin.names.contains(&server_tool));
        }
        let mut sorted = on.builtin.names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, on.builtin.names);

        assert_eq!(on, CloudToolCatalog::new(true, Vec::new()));
        assert_ne!(on.revision(), off.revision());

        let tool =
            serde_json::json!({"alias":"mcp__docs__search","input_schema":{"type":"object"}});
        let with_tool = CloudToolCatalog::new(true, vec![tool.clone()]);
        assert!(with_tool.has_external() && !on.has_external());
        assert_eq!(
            serde_json::to_value(&with_tool).unwrap()["external"][0],
            tool
        );
        assert_ne!(with_tool.revision(), on.revision());
    }
}
