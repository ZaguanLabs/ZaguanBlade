//! Versioned protocol result plus a bounded legacy text projection. No fetching,
//! rendering, caching or transcript persistence of external content happens here.
use super::{
    permissions::{CallReview, CallScope, CallTarget},
    RuntimeError,
};
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

const MAX_RESULT_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_DEPTH: usize = 32;
pub(super) const MAX_ARGUMENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    NotStarted,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallFailure {
    pub code: RuntimeError,
    /// Unknown means side effects may have occurred. Never automatically retry.
    pub outcome: CallOutcome,
}
impl From<RuntimeError> for CallFailure {
    fn from(code: RuntimeError) -> Self {
        Self {
            code,
            outcome: CallOutcome::NotStarted,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct McpCallResult {
    pub schema_version: u32,
    pub scope: CallScope,
    pub target: CallTarget,
    pub integration_id: Uuid,
    pub tool_name: String,
    pub is_error: bool,
    /// Full SDK-supported content, structuredContent and metadata, bounded below.
    pub result: Value,
    pub text: String,
    pub text_truncated: bool,
    /// Text projections cannot represent these blocks; keep the envelope.
    pub non_text_blocks: usize,
}

pub(super) fn bounded_json(value: &Value, max_bytes: usize) -> Result<(), RuntimeError> {
    fn depth(value: &Value, level: usize) -> Result<(), RuntimeError> {
        if level > MAX_DEPTH {
            return Err(RuntimeError::OutputLimit);
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    depth(item, level + 1)?;
                }
            }
            Value::Object(items) => {
                for item in items.values() {
                    depth(item, level + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    depth(value, 0)?;
    // Serialize to a bounded counter rather than allocating another large copy.
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or(std::io::ErrorKind::FileTooLarge)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Budget(max_bytes), value).map_err(|_| RuntimeError::OutputLimit)
}

impl McpCallResult {
    pub(super) fn normalize(
        review: &CallReview,
        result: CallToolResult,
    ) -> Result<Self, RuntimeError> {
        let value = serde_json::to_value(&result).map_err(|_| RuntimeError::ProtocolFailed)?;
        bounded_json(&value, MAX_RESULT_BYTES)?;
        let mut text = String::new();
        let mut truncated = false;
        let mut non_text_blocks = 0;
        let mut append = |part: &str| {
            let available = MAX_TEXT_BYTES.saturating_sub(text.len());
            let mut end = part.len().min(available);
            while !part.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&part[..end]);
            truncated |= end < part.len();
        };
        if let Some(content) = value["content"].as_array() {
            for block in content {
                if let Some(body) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|_| block["type"] == "text")
                {
                    append(body);
                    append("\n");
                } else {
                    non_text_blocks += 1;
                    // Machine-readable projection marker; the UI must render a
                    // translated representation from the preserved envelope.
                    append("{\"mcp_non_text_content\":true}\n");
                }
            }
        }
        if let Some(structured) = result.structured_content.as_ref() {
            append(&serde_json::to_string(structured).map_err(|_| RuntimeError::ProtocolFailed)?);
        }
        Ok(Self {
            schema_version: 1,
            scope: review.scope.clone(),
            target: review.target.clone(),
            integration_id: review.integration_id,
            tool_name: review.tool_name.clone(),
            is_error: result.is_error.unwrap_or(false),
            result: value,
            text,
            text_truncated: truncated,
            non_text_blocks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn review() -> CallReview {
        CallReview {
            request_id: Uuid::new_v4(),
            scope: CallScope {
                workspace: super::super::identity::WorkspaceIdentity::new(std::path::Path::new(
                    "/workspace",
                )),
                conversation_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                call_id: "call".into(),
            },
            target: CallTarget {
                connection_id: Uuid::new_v4(),
                alias: "alias".into(),
                catalog_revision: "revision".into(),
            },
            integration_id: Uuid::new_v4(),
            server_name: "server".into(),
            tool_name: "search".into(),
            arguments: json!({}),
        }
    }
    #[test]
    fn rich_result_preserves_structured_content_binary_blocks_metadata_and_error() {
        let wire = json!({"content":[{"type":"text","text":"España"},{"type":"image","data":"YWJj","mimeType":"image/png"},
            {"type":"resource_link","uri":"https://example.invalid/private","name":"private"}],
            "structuredContent":{"answer":42},"isError":true,"_meta":{"source":"fixture"}});
        let result =
            McpCallResult::normalize(&review(), serde_json::from_value(wire.clone()).unwrap())
                .unwrap();
        assert_eq!(result.result, wire);
        assert!(result.is_error);
        assert_eq!(result.non_text_blocks, 2);
        assert!(result.text.contains("España"));
        assert!(result.text.contains("42"));
        assert!(!result.text.contains("YWJj"));
        assert!(!result.text.contains("https://"));
        assert!(!result.text_truncated);
    }
    #[test]
    fn text_projection_is_utf8_safe_and_oversized_or_deep_payloads_fail_explicitly() {
        let result = McpCallResult::normalize(
            &review(),
            serde_json::from_value(json!({"content":[{"type":"text","text":"🦀".repeat(5000)}]}))
                .unwrap(),
        )
        .unwrap();
        assert!(result.text.len() <= MAX_TEXT_BYTES);
        assert!(result.text_truncated);
        assert_eq!(
            result.result["content"][0]["text"].as_str().unwrap().len(),
            20000
        );
        assert_eq!(
            bounded_json(&json!({"a":"b"}), 8),
            Err(RuntimeError::OutputLimit)
        );
        let mut deep = json!(null);
        for _ in 0..40 {
            deep = json!([deep]);
        }
        assert_eq!(
            bounded_json(&deep, MAX_RESULT_BYTES),
            Err(RuntimeError::OutputLimit)
        );
        let large = CallToolResult::structured(json!({"data":"x".repeat(MAX_RESULT_BYTES)}));
        assert!(matches!(
            McpCallResult::normalize(&review(), large),
            Err(RuntimeError::OutputLimit)
        ));
    }
}
