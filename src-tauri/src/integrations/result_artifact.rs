//! Machine-local MCP artifacts; only UUID identifiers enter paths. Transcript
//! projections carry references, never binary payloads or repeated rich envelopes.
use super::{call_result::McpCallResult, RuntimeError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
const MAX_ARTIFACT_BYTES: usize = 1024 * 1024 + 64 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub conversation_id: Uuid,
    pub turn_id: Uuid,
    pub artifact_id: Uuid,
}
fn path(directory: &Path, workspace: &str, reference: &ArtifactRef) -> PathBuf {
    directory
        .join("mcp-results")
        .join(workspace)
        .join(reference.conversation_id.to_string())
        .join(reference.turn_id.to_string())
        .join(format!("{}.json", reference.artifact_id))
}
pub fn save(directory: &Path, result: &McpCallResult) -> Result<ArtifactRef, RuntimeError> {
    save_value(
        directory,
        &result.scope,
        &serde_json::to_value(result).map_err(|_| RuntimeError::OutputLimit)?,
    )
}
pub(super) fn save_value(
    directory: &Path,
    scope: &super::permissions::CallScope,
    value: &Value,
) -> Result<ArtifactRef, RuntimeError> {
    let reference = ArtifactRef {
        conversation_id: scope.conversation_id,
        turn_id: scope.turn_id,
        artifact_id: Uuid::new_v4(),
    };
    let path = path(directory, &scope.workspace.workspace_id, &reference);
    let bytes = serde_json::to_vec(value).map_err(|_| RuntimeError::OutputLimit)?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(RuntimeError::OutputLimit);
    }
    fs::create_dir_all(path.parent().ok_or(RuntimeError::ArtifactUnavailable)?)
        .map_err(|_| RuntimeError::ArtifactUnavailable)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| RuntimeError::ArtifactUnavailable)?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(path);
        return Err(RuntimeError::ArtifactUnavailable);
    }
    Ok(reference)
}
pub fn read(
    directory: &Path,
    workspace: &str,
    reference: &ArtifactRef,
) -> Result<Value, RuntimeError> {
    let file = fs::File::open(path(directory, workspace, reference))
        .map_err(|_| RuntimeError::ArtifactUnavailable)?;
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RuntimeError::ArtifactUnavailable)?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(RuntimeError::OutputLimit);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| RuntimeError::ArtifactUnavailable)?;
    if value["schema_version"] != 1
        || value["scope"]["workspace"]["workspace_id"] != workspace
        || value["scope"]["conversation_id"] != reference.conversation_id.to_string()
        || value["scope"]["turn_id"] != reference.turn_id.to_string()
    {
        return Err(RuntimeError::ArtifactUnavailable);
    }
    Ok(value)
}

impl super::runtime::IntegrationRuntime {
    pub(crate) fn read_result_artifact(
        &self,
        workspace: &str,
        reference: &ArtifactRef,
    ) -> Result<Value, RuntimeError> {
        read(&self.directory, workspace, reference)
    }
}
