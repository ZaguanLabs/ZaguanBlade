//! Context Assembly for AI Prompts
//!
//! Intelligently assembles code context for AI models by selecting
//! relevant symbols, files, and related code based on the current
//! cursor position and user intent.
//!
//! Strategies:
//! - Symbol-based: Include related definitions, usages, and types
//! - File-based: Include relevant portions of open files
//! - Semantic: Use symbol relationships for smart selection

mod assembler;
mod budget;
mod strategy;

pub use assembler::{AssembledContext, ContextAssembler};
pub use budget::{BudgetAllocation, TokenBudget};
pub use strategy::{ContextStrategy, StrategyConfig};
