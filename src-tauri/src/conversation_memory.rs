//! Producers for the local cross-session memory that was fully wired but never
//! populated: `Message.code_references` and `ConversationArtifact.moments`.
//!
//! Both are stored in `.zblade/artifacts` and indexed by `local_index` (FTS5).
//! Moments feed back into every future turn via `context_pack::collect_memories`
//! → `LocalArtifactStore::search_moments`, so PRECISION is the safety mechanism:
//! a false-positive "decision" would be injected into all later context packs.
//! Extraction is therefore deliberately conservative —
//!   * `extract_code_references`: only `path.ext[:line[-line]]` mentions whose
//!     path is a recognized code file (`file_lang::is_code_file`).
//!   * `extract_moments`: a sentence must contain BOTH a decision marker AND a
//!     rationale marker (because/since/…) — the reason gate ported from
//!     code-context-engine's zero-LLM decision extractor — plus dedup + caps.
//! Everything here is pure and unit-tested; no LLM, no network (keeps ZB light).

use std::collections::HashSet;
use std::path::PathBuf;

use crate::local_artifacts::{CodeReference, Message, Moment};

/// Max code references captured per message (bounds noise + storage).
const MAX_REFS_PER_MESSAGE: usize = 12;
/// Max decision moments captured per conversation (bounds context-pack cost).
const MAX_MOMENTS_PER_CONVERSATION: usize = 12;
/// Sentence length window for decision detection (chars).
const MIN_DECISION_LEN: usize = 20;
const MAX_DECISION_LEN: usize = 400;

lazy_static::lazy_static! {
    /// `path.ext`, optionally `:line` or `:line-line` (hyphen or en-dash).
    static ref CODE_REF_RE: regex::Regex = regex::Regex::new(
        r"([A-Za-z0-9_./\-]+\.[A-Za-z][A-Za-z0-9]{0,9})(?::(\d{1,7})(?:[-\x{2013}](\d{1,7}))?)?"
    ).expect("valid code-reference regex");

    /// Markers that a choice/decision is being stated.
    static ref DECISION_RE: regex::Regex = regex::Regex::new(
        r"(?i)\b(decided to|decided that|decision (?:is|was)|chose to|chose|choosing|switch(?:ed|ing)? to|going with|go with|let'?s use|we'?ll use|we will use|opt(?:ed|ing)? for|adopt(?:ed|ing)?|instead of|prefer(?:red|ring)?)\b"
    ).expect("valid decision regex");

    /// Markers that a rationale is present. The mandatory reason gate.
    static ref REASON_RE: regex::Regex = regex::Regex::new(
        r"(?i)\b(because|since|so that|in order to|to avoid|due to|as it|reason(?:s)? (?:is|are|being)|rationale)\b"
    ).expect("valid reason regex");
}

/// Extract `file:line` references mentioned in a message body. Stores locators
/// only (reference-not-text); dedups on (file, start, end).
pub fn extract_code_references(content: &str) -> Vec<CodeReference> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for caps in CODE_REF_RE.captures_iter(content) {
        let file = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        if file.is_empty() || !crate::file_lang::is_code_file(&PathBuf::from(file)) {
            continue;
        }
        let start = caps
            .get(2)
            .and_then(|m| m.as_str().parse::<i32>().ok())
            .unwrap_or(0);
        let end = caps
            .get(3)
            .and_then(|m| m.as_str().parse::<i32>().ok())
            .unwrap_or(start);
        if !seen.insert((file.to_string(), start, end)) {
            continue;
        }
        refs.push(CodeReference {
            file: file.to_string(),
            lines: (start, end),
            git_hash: None,
            context: None,
            diff: None,
        });
        if refs.len() >= MAX_REFS_PER_MESSAGE {
            break;
        }
    }
    refs
}

/// Extract decision moments from a conversation. A sentence qualifies only when
/// it states a decision AND gives a reason (the precision gate); the moment
/// carries the sentence as `content`, the rationale as `context`, and inherits
/// the message's code references. Dedups on normalized content; capped.
pub fn extract_moments(messages: &[Message]) -> Vec<Moment> {
    let mut moments = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for message in messages {
        for sentence in split_sentences(&message.content) {
            let trimmed = sentence.trim();
            if trimmed.len() < MIN_DECISION_LEN || trimmed.len() > MAX_DECISION_LEN {
                continue;
            }
            if !DECISION_RE.is_match(trimmed) || !REASON_RE.is_match(trimmed) {
                continue;
            }
            let content = truncate_chars(trimmed, 200);
            let dedup_key = content.to_ascii_lowercase();
            if !seen.insert(dedup_key) {
                continue;
            }
            let context = REASON_RE
                .find(trimmed)
                .map(|m| truncate_chars(trimmed[m.start()..].trim(), 200));
            moments.push(Moment {
                id: format!("{}-decision-{}", message.id, moments.len()),
                moment_type: "decision".to_string(),
                content,
                context,
                message_id: Some(message.id.clone()),
                timestamp: message.timestamp.clone(),
                tags: vec!["decision".to_string()],
                code_references: message.code_references.clone(),
                relevance_score: 0.6,
            });
            if moments.len() >= MAX_MOMENTS_PER_CONVERSATION {
                return moments;
            }
        }
    }
    moments
}

/// Split text into rough sentences on terminators and newlines. Good enough for
/// decision detection — the decision+reason co-occurrence does the real work.
fn split_sentences(text: &str) -> Vec<&str> {
    text.split(|c| matches!(c, '.' | '!' | '?' | '\n' | '\r' | ';'))
        .filter(|s| !s.trim().is_empty())
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}\u{2026}", truncated.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, role: &str, content: &str) -> Message {
        Message {
            id: id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: "2026-07-03T00:00:00Z".to_string(),
            code_references: Vec::new(),
        }
    }

    #[test]
    fn extracts_file_line_references() {
        let refs = extract_code_references(
            "Fixed it in src/tools.rs:3914 and updated src/language_service/service.rs:3022-3137.",
        );
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].file, "src/tools.rs");
        assert_eq!(refs[0].lines, (3914, 3914));
        assert_eq!(refs[1].file, "src/language_service/service.rs");
        assert_eq!(refs[1].lines, (3022, 3137));
    }

    #[test]
    fn ignores_non_code_and_version_like_tokens() {
        // "e.g." / "v1.2" / prose should not be mistaken for code refs.
        let refs = extract_code_references("e.g. bump to v1.2 as discussed, see the README overview");
        assert!(refs.is_empty(), "unexpected refs: {refs:?}");
    }

    #[test]
    fn dedups_repeated_references() {
        let refs = extract_code_references("touch src/a.rs:1 then src/a.rs:1 again");
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn decision_needs_both_marker_and_reason() {
        // Decision + reason → captured.
        let hit = extract_moments(&[msg(
            "msg_0",
            "assistant",
            "We decided to use SQLite because it needs no server and simplifies local storage.",
        )]);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].moment_type, "decision");
        assert!(hit[0].content.contains("decided to use SQLite"));
        assert!(hit[0].context.as_deref().unwrap().starts_with("because"));

        // Decision WITHOUT a reason → not captured (precision gate).
        let no_reason = extract_moments(&[msg("m", "assistant", "We decided to use SQLite for this.")]);
        assert!(no_reason.is_empty());

        // Reason WITHOUT a decision marker → not captured.
        let no_decision =
            extract_moments(&[msg("m", "user", "The tests pass because the index is fresh now.")]);
        assert!(no_decision.is_empty());
    }

    #[test]
    fn decision_moment_inherits_message_code_references() {
        let mut m = msg(
            "msg_5",
            "assistant",
            "Switched to xxh3 because DefaultHasher is not version-stable.",
        );
        m.code_references = vec![CodeReference {
            file: "src/stable_hash.rs".to_string(),
            lines: (1, 1),
            git_hash: None,
            context: None,
            diff: None,
        }];
        let moments = extract_moments(&[m]);
        assert_eq!(moments.len(), 1);
        assert_eq!(moments[0].code_references.len(), 1);
        assert_eq!(moments[0].code_references[0].file, "src/stable_hash.rs");
        assert_eq!(moments[0].message_id.as_deref(), Some("msg_5"));
    }

    #[test]
    fn dedups_identical_decisions_and_caps() {
        let mut messages = Vec::new();
        for i in 0..30 {
            messages.push(msg(
                &format!("m{i}"),
                "assistant",
                "We chose Postgres because it scales better than SQLite here.",
            ));
        }
        let moments = extract_moments(&messages);
        // Identical content dedups to a single moment.
        assert_eq!(moments.len(), 1);
    }
}
