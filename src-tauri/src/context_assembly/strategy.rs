//! Context assembly strategies
//!
//! Different strategies for selecting and prioritizing code context.

use serde::{Deserialize, Serialize};

/// Context assembly strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    /// Focus on the current file with expanded definitions
    Focused,
    /// Include related files and symbols (default)
    Balanced,
    /// Include as much context as possible
    Comprehensive,
    /// Minimal context for fast responses
    Minimal,
    /// Custom configuration
    Custom,
}

impl Default for ContextStrategy {
    fn default() -> Self {
        Self::Balanced
    }
}

/// Strategy configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// How much to expand from cursor position (lines in each direction)
    pub cursor_expansion: usize,
    /// Include symbol definitions (go-to-definition targets)
    pub include_definitions: bool,
    /// Include symbol references (find-references targets)
    pub include_references: bool,
    /// Include related types (type definitions used in current scope)
    pub include_types: bool,
    /// Include imports from the current file
    pub include_imports: bool,
    /// Maximum files to include from open files
    pub max_open_files: usize,
    /// Priority weights for different context types
    pub weights: ContextWeights,
}

/// Priority weights for context selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextWeights {
    /// Weight for active file content
    pub active_file: f32,
    /// Weight for symbol definitions
    pub definitions: f32,
    /// Weight for symbol references
    pub references: f32,
    /// Weight for type definitions
    pub types: f32,
    /// Weight for imports
    pub imports: f32,
    /// Weight for open files
    pub open_files: f32,
}

impl Default for ContextWeights {
    fn default() -> Self {
        Self {
            active_file: 1.0,
            definitions: 0.8,
            references: 0.5,
            types: 0.6,
            imports: 0.3,
            open_files: 0.4,
        }
    }
}

impl StrategyConfig {
    /// Get config for Focused strategy
    pub fn focused() -> Self {
        Self {
            cursor_expansion: 50,
            include_definitions: true,
            include_references: false,
            include_types: true,
            include_imports: true,
            max_open_files: 0,
            weights: ContextWeights {
                active_file: 1.0,
                definitions: 0.9,
                references: 0.0,
                types: 0.7,
                imports: 0.3,
                open_files: 0.0,
            },
        }
    }

    /// Get config for Balanced strategy
    pub fn balanced() -> Self {
        Self {
            cursor_expansion: 100,
            include_definitions: true,
            include_references: true,
            include_types: true,
            include_imports: true,
            max_open_files: 3,
            weights: ContextWeights::default(),
        }
    }

    /// Get config for Comprehensive strategy
    pub fn comprehensive() -> Self {
        Self {
            cursor_expansion: 200,
            include_definitions: true,
            include_references: true,
            include_types: true,
            include_imports: true,
            max_open_files: 10,
            weights: ContextWeights {
                active_file: 1.0,
                definitions: 0.9,
                references: 0.8,
                types: 0.8,
                imports: 0.5,
                open_files: 0.7,
            },
        }
    }

    /// Get config for Minimal strategy
    pub fn minimal() -> Self {
        Self {
            cursor_expansion: 20,
            include_definitions: false,
            include_references: false,
            include_types: false,
            include_imports: false,
            max_open_files: 0,
            weights: ContextWeights {
                active_file: 1.0,
                definitions: 0.0,
                references: 0.0,
                types: 0.0,
                imports: 0.0,
                open_files: 0.0,
            },
        }
    }

    /// Get config for a given strategy
    pub fn for_strategy(strategy: ContextStrategy) -> Self {
        match strategy {
            ContextStrategy::Focused => Self::focused(),
            ContextStrategy::Balanced => Self::balanced(),
            ContextStrategy::Comprehensive => Self::comprehensive(),
            ContextStrategy::Minimal => Self::minimal(),
            ContextStrategy::Custom => Self::balanced(), // Default to balanced for custom
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self::balanced()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_configs() {
        let focused = StrategyConfig::focused();
        assert!(focused.cursor_expansion < 100);
        assert!(!focused.include_references);

        let comprehensive = StrategyConfig::comprehensive();
        assert!(comprehensive.cursor_expansion >= 200);
        assert!(comprehensive.include_references);
    }

    #[test]
    fn test_weights() {
        let weights = ContextWeights::default();
        assert!(weights.active_file > weights.references);
        assert!(weights.definitions > weights.open_files);
    }
}
