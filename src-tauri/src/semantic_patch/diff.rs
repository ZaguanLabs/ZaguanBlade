//! Diff Generation
//!
//! Generates unified diffs for previewing and reviewing patches
//! before they are applied.

use serde::{Deserialize, Serialize};
use similar::{Algorithm, ChangeTag, TextDiff};

/// A hunk in a unified diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line in old file (1-indexed)
    pub old_start: usize,
    /// Number of lines in old file
    pub old_count: usize,
    /// Starting line in new file (1-indexed)
    pub new_start: usize,
    /// Number of lines in new file
    pub new_count: usize,
    /// The diff lines with prefixes (+/-/space)
    pub lines: Vec<DiffLine>,
}

/// A line in a diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    /// Type of line
    pub kind: DiffLineKind,
    /// The content (without prefix)
    pub content: String,
    /// Original line number (for context/removed)
    pub old_line: Option<usize>,
    /// New line number (for context/added)
    pub new_line: Option<usize>,
}

/// Kind of diff line
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

impl DiffLine {
    /// Get the prefix character for this line kind
    pub fn prefix(&self) -> char {
        match self.kind {
            DiffLineKind::Context => ' ',
            DiffLineKind::Added => '+',
            DiffLineKind::Removed => '-',
        }
    }

    /// Format as a diff line string
    pub fn to_string_with_prefix(&self) -> String {
        format!("{}{}", self.prefix(), self.content)
    }
}

impl DiffHunk {
    /// Format the hunk header
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        )
    }

    /// Format the entire hunk as a string
    pub fn to_string(&self) -> String {
        let mut result = self.header();
        result.push('\n');
        for line in &self.lines {
            result.push_str(&line.to_string_with_prefix());
            result.push('\n');
        }
        result
    }
}

/// Generate a unified diff between two strings
pub fn generate_diff(old_content: &str, new_content: &str, context_lines: usize) -> Vec<DiffHunk> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Histogram)
        .diff_lines(old_content, new_content);

    diff.grouped_ops(context_lines)
        .into_iter()
        .filter_map(|ops| {
            let first = ops.first()?;
            let last = ops.last()?;
            let old_range = first.old_range().start..last.old_range().end;
            let new_range = first.new_range().start..last.new_range().end;
            let mut hunk = DiffHunk {
                old_start: old_range.start + 1,
                old_count: old_range.len(),
                new_start: new_range.start + 1,
                new_count: new_range.len(),
                lines: Vec::new(),
            };

            for op in ops {
                for change in diff.iter_changes(&op) {
                    match change.tag() {
                        ChangeTag::Equal => hunk.lines.push(DiffLine {
                            kind: DiffLineKind::Context,
                            content: change.value().trim_end_matches('\n').to_string(),
                            old_line: change.old_index().map(|index| index + 1),
                            new_line: change.new_index().map(|index| index + 1),
                        }),
                        ChangeTag::Delete => hunk.lines.push(DiffLine {
                            kind: DiffLineKind::Removed,
                            content: change.value().trim_end_matches('\n').to_string(),
                            old_line: change.old_index().map(|index| index + 1),
                            new_line: None,
                        }),
                        ChangeTag::Insert => hunk.lines.push(DiffLine {
                            kind: DiffLineKind::Added,
                            content: change.value().trim_end_matches('\n').to_string(),
                            old_line: None,
                            new_line: change.new_index().map(|index| index + 1),
                        }),
                    }
                }
            }

            if hunk
                .lines
                .iter()
                .any(|line| line.kind != DiffLineKind::Context)
            {
                Some(hunk)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";

        let hunks = generate_diff(old, new, 1);

        assert_eq!(hunks.len(), 1);
        let hunk = &hunks[0];
        assert!(hunk
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Removed && l.content == "line2"));
        assert!(hunk
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Added && l.content == "modified"));
    }

    #[test]
    fn test_no_changes() {
        let content = "line1\nline2\nline3";
        let hunks = generate_diff(content, content, 1);

        // No changes means no hunks (all context)
        assert!(
            hunks.is_empty()
                || hunks
                    .iter()
                    .all(|h| h.lines.iter().all(|l| l.kind == DiffLineKind::Context))
        );
    }

    #[test]
    fn test_addition() {
        let old = "line1\nline3";
        let new = "line1\nline2\nline3";

        let hunks = generate_diff(old, new, 1);

        assert!(!hunks.is_empty());
        assert!(hunks[0]
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Added && l.content == "line2"));
    }

    #[test]
    fn test_deletion() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline3";

        let hunks = generate_diff(old, new, 1);

        assert!(!hunks.is_empty());
        assert!(hunks[0]
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Removed && l.content == "line2"));
    }
}
