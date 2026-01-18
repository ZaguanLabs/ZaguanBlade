//! Diff Generation
//!
//! Generates unified diffs for previewing and reviewing patches
//! before they are applied.

use serde::{Deserialize, Serialize};

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
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    // Simple LCS-based diff algorithm
    let lcs = compute_lcs(&old_lines, &new_lines);

    // Build diff operations
    let ops = build_diff_ops(&old_lines, &new_lines, &lcs);

    // Group into hunks with context
    build_hunks(&old_lines, &new_lines, &ops, context_lines)
}

/// Diff operation
#[derive(Debug, Clone)]
enum DiffOp {
    Keep(usize, usize), // old_idx, new_idx
    Remove(usize),      // old_idx
    Add(usize),         // new_idx
}

/// Compute longest common subsequence indices
fn compute_lcs<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(usize, usize)> {
    let m = old.len();
    let n = new.len();

    if m == 0 || n == 0 {
        return vec![];
    }

    // DP table for LCS length
    let mut dp = vec![vec![0usize; n + 1]; m + 1];

    for (i, old_line) in old.iter().enumerate() {
        for (j, new_line) in new.iter().enumerate() {
            if old_line == new_line {
                dp[i + 1][j + 1] = dp[i][j] + 1;
            } else {
                dp[i + 1][j + 1] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    // Backtrack to find LCS
    let mut lcs = Vec::new();
    let mut i = m;
    let mut j = n;

    while i > 0 && j > 0 {
        if old[i - 1] == new[j - 1] {
            lcs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] > dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    lcs.reverse();
    lcs
}

/// Build diff operations from LCS
fn build_diff_ops(old: &[&str], new: &[&str], lcs: &[(usize, usize)]) -> Vec<DiffOp> {
    let mut ops = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;

    for &(lcs_old, lcs_new) in lcs {
        // Add removed lines before this LCS element
        while old_idx < lcs_old {
            ops.push(DiffOp::Remove(old_idx));
            old_idx += 1;
        }

        // Add new lines before this LCS element
        while new_idx < lcs_new {
            ops.push(DiffOp::Add(new_idx));
            new_idx += 1;
        }

        // Add the matching line
        ops.push(DiffOp::Keep(old_idx, new_idx));
        old_idx += 1;
        new_idx += 1;
    }

    // Handle remaining lines
    while old_idx < old.len() {
        ops.push(DiffOp::Remove(old_idx));
        old_idx += 1;
    }

    while new_idx < new.len() {
        ops.push(DiffOp::Add(new_idx));
        new_idx += 1;
    }

    ops
}

/// Build hunks from diff operations
fn build_hunks(old: &[&str], new: &[&str], ops: &[DiffOp], context_lines: usize) -> Vec<DiffHunk> {
    if ops.is_empty() {
        return vec![];
    }

    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut last_change_idx = 0;

    for (idx, op) in ops.iter().enumerate() {
        let is_change = !matches!(op, DiffOp::Keep(_, _));

        if is_change {
            // Check if we need to start a new hunk or continue existing
            if current_hunk.is_none() || idx > last_change_idx + context_lines * 2 + 1 {
                // Finish previous hunk if exists
                if let Some(hunk) = current_hunk.take() {
                    hunks.push(hunk);
                }

                // Start new hunk with context before
                let start_idx = idx.saturating_sub(context_lines);
                let (old_start, new_start) = get_line_numbers_at_idx(ops, start_idx);

                current_hunk = Some(DiffHunk {
                    old_start: old_start + 1, // 1-indexed
                    old_count: 0,
                    new_start: new_start + 1,
                    new_count: 0,
                    lines: Vec::new(),
                });

                // Add context lines before
                for ctx_idx in start_idx..idx {
                    if let Some(ref mut hunk) = current_hunk {
                        add_op_to_hunk(hunk, &ops[ctx_idx], old, new);
                    }
                }
            }

            last_change_idx = idx;
        }

        // Add to current hunk
        if let Some(ref mut hunk) = current_hunk {
            add_op_to_hunk(hunk, op, old, new);

            // Add context after if this is a change
            if is_change {
                let context_end = (idx + context_lines + 1).min(ops.len());
                for ctx_idx in (idx + 1)..context_end {
                    if matches!(ops[ctx_idx], DiffOp::Keep(_, _)) {
                        break; // Will be added in next iteration
                    }
                }
            }
        }
    }

    // Finish last hunk
    if let Some(hunk) = current_hunk {
        if !hunk.lines.is_empty() {
            hunks.push(hunk);
        }
    }

    hunks
}

fn get_line_numbers_at_idx(ops: &[DiffOp], idx: usize) -> (usize, usize) {
    let mut old_line = 0;
    let mut new_line = 0;

    for op in ops.iter().take(idx) {
        match op {
            DiffOp::Keep(_, _) => {
                old_line += 1;
                new_line += 1;
            }
            DiffOp::Remove(_) => old_line += 1,
            DiffOp::Add(_) => new_line += 1,
        }
    }

    (old_line, new_line)
}

fn add_op_to_hunk(hunk: &mut DiffHunk, op: &DiffOp, old: &[&str], new: &[&str]) {
    match op {
        DiffOp::Keep(old_idx, new_idx) => {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                content: old[*old_idx].to_string(),
                old_line: Some(*old_idx + 1),
                new_line: Some(*new_idx + 1),
            });
            hunk.old_count += 1;
            hunk.new_count += 1;
        }
        DiffOp::Remove(old_idx) => {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Removed,
                content: old[*old_idx].to_string(),
                old_line: Some(*old_idx + 1),
                new_line: None,
            });
            hunk.old_count += 1;
        }
        DiffOp::Add(new_idx) => {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Added,
                content: new[*new_idx].to_string(),
                old_line: None,
                new_line: Some(*new_idx + 1),
            });
            hunk.new_count += 1;
        }
    }
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
