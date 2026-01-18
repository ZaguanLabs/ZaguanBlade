//! Token budget management
//!
//! Manages token allocation for context assembly to ensure
//! we don't exceed model context limits.

use serde::{Deserialize, Serialize};

/// Token budget configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Total tokens available for context
    pub total: usize,
    /// Reserved for system prompt
    pub system_reserve: usize,
    /// Reserved for user message
    pub user_reserve: usize,
    /// Reserved for response generation
    pub response_reserve: usize,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            total: 128_000,          // Default to 128K context
            system_reserve: 4_000,   // System prompt
            user_reserve: 2_000,     // User's message
            response_reserve: 8_000, // Response generation
        }
    }
}

impl TokenBudget {
    /// Create a budget for a smaller context window
    pub fn small() -> Self {
        Self {
            total: 8_000,
            system_reserve: 1_000,
            user_reserve: 500,
            response_reserve: 2_000,
        }
    }

    /// Create a budget for medium context
    pub fn medium() -> Self {
        Self {
            total: 32_000,
            system_reserve: 2_000,
            user_reserve: 1_000,
            response_reserve: 4_000,
        }
    }

    /// Create a budget for large context
    pub fn large() -> Self {
        Self {
            total: 128_000,
            system_reserve: 4_000,
            user_reserve: 2_000,
            response_reserve: 8_000,
        }
    }

    /// Calculate available tokens for code context
    pub fn available_for_context(&self) -> usize {
        self.total
            .saturating_sub(self.system_reserve)
            .saturating_sub(self.user_reserve)
            .saturating_sub(self.response_reserve)
    }

    /// Create custom budget
    pub fn custom(total: usize) -> Self {
        // Allocate roughly: 5% system, 2.5% user, 10% response, 82.5% context
        Self {
            total,
            system_reserve: total / 20,
            user_reserve: total / 40,
            response_reserve: total / 10,
        }
    }
}

/// Allocation of budget across context types
#[derive(Debug, Clone, Default)]
pub struct BudgetAllocation {
    /// Tokens used for active file context
    pub active_file: usize,
    /// Tokens used for symbol definitions
    pub definitions: usize,
    /// Tokens used for symbol usages/references
    pub references: usize,
    /// Tokens used for related types
    pub related_types: usize,
    /// Tokens used for imports/dependencies
    pub imports: usize,
    /// Tokens used for open files context
    pub open_files: usize,
}

impl BudgetAllocation {
    /// Calculate total tokens used
    pub fn total(&self) -> usize {
        self.active_file
            + self.definitions
            + self.references
            + self.related_types
            + self.imports
            + self.open_files
    }

    /// Check if under a given limit
    pub fn is_under(&self, limit: usize) -> bool {
        self.total() < limit
    }

    /// Calculate remaining budget
    pub fn remaining(&self, budget: &TokenBudget) -> usize {
        budget.available_for_context().saturating_sub(self.total())
    }
}

/// Simple token estimator (approximation)
pub fn estimate_tokens(text: &str) -> usize {
    // Rough estimation: ~4 characters per token for code
    // This is a simplification; real tokenization varies by model
    (text.len() + 3) / 4
}

/// Truncate text to fit within token budget
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> &str {
    let estimated_chars = max_tokens * 4;
    if text.len() <= estimated_chars {
        text
    } else {
        // Try to truncate at a line boundary
        let truncated = &text[..estimated_chars.min(text.len())];
        if let Some(last_newline) = truncated.rfind('\n') {
            &text[..last_newline]
        } else {
            truncated
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_budget() {
        let budget = TokenBudget::default();
        assert_eq!(budget.total, 128_000);
        assert!(budget.available_for_context() > 100_000);
    }

    #[test]
    fn test_budget_allocation() {
        let budget = TokenBudget::medium();
        let mut alloc = BudgetAllocation::default();

        alloc.active_file = 5000;
        alloc.definitions = 3000;

        assert_eq!(alloc.total(), 8000);
        assert!(alloc.remaining(&budget) > 0);
    }

    #[test]
    fn test_token_estimation() {
        let text = "function hello() { console.log('Hello'); }";
        let tokens = estimate_tokens(text);
        // ~42 chars / 4 = ~10-11 tokens
        assert!(tokens >= 10 && tokens <= 15);
    }

    #[test]
    fn test_truncation() {
        let text = "line1\nline2\nline3\nline4\nline5";
        let truncated = truncate_to_tokens(text, 4); // ~16 chars
        assert!(truncated.len() <= 20);
    }
}
