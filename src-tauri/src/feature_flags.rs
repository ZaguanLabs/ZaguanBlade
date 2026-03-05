//! Feature Flags for Headless Migration
//!
//! Controls the Strangler Fig migration pattern, allowing gradual transition
//! from frontend-authoritative to backend-authoritative state management.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Feature flags controlling backend authority for different domains.
/// Uses atomic bools for thread-safe access without locks.
pub struct FeatureFlags {
    /// When true, editor state (active file, cursor, selection) is authoritative in Rust.
    /// Frontend should react to EditorEvent rather than owning state.
    editor_backend_authority: AtomicBool,

    /// When true, tab state (open tabs, tab order) is authoritative in Rust.
    /// Frontend should react to TabEvent rather than owning state.
    tabs_backend_authority: AtomicBool,

    /// When true, expose advanced composite tools (read_many_files, batch,
    /// codebase_investigator) for supported model families.
    composite_tools_enabled: AtomicBool,

    /// When true, grep_search timeout bounds are enforced in executor.
    grep_timeout_enforced: AtomicBool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            // Backend authority enabled for editor state
            editor_backend_authority: AtomicBool::new(true),
            // Backend authority enabled for tab state
            tabs_backend_authority: AtomicBool::new(true),
            // Composite tool support enabled (still model-family gated)
            composite_tools_enabled: AtomicBool::new(true),
            // Rollout flag: disabled by default until validated in staging
            grep_timeout_enforced: AtomicBool::new(false),
        }
    }
}

impl FeatureFlags {
    pub fn new() -> Self {
        Self::default()
    }

    // Editor authority

    pub fn editor_backend_authority(&self) -> bool {
        self.editor_backend_authority.load(Ordering::Relaxed)
    }

    pub fn set_editor_backend_authority(&self, value: bool) {
        self.editor_backend_authority.store(value, Ordering::Relaxed);
    }

    // Tabs authority

    pub fn tabs_backend_authority(&self) -> bool {
        self.tabs_backend_authority.load(Ordering::Relaxed)
    }

    pub fn set_tabs_backend_authority(&self, value: bool) {
        self.tabs_backend_authority.store(value, Ordering::Relaxed);
    }

    // Composite tools

    pub fn composite_tools_enabled(&self) -> bool {
        self.composite_tools_enabled.load(Ordering::Relaxed)
    }

    pub fn set_composite_tools_enabled(&self, value: bool) {
        self.composite_tools_enabled.store(value, Ordering::Relaxed);
    }

    // grep_search timeout enforcement

    pub fn grep_timeout_enforced(&self) -> bool {
        self.grep_timeout_enforced.load(Ordering::Relaxed)
    }

    pub fn set_grep_timeout_enforced(&self, value: bool) {
        self.grep_timeout_enforced.store(value, Ordering::Relaxed);
    }

    /// Returns a serializable snapshot of current flag values
    pub fn snapshot(&self) -> FeatureFlagsSnapshot {
        FeatureFlagsSnapshot {
            editor_backend_authority: self.editor_backend_authority(),
            tabs_backend_authority: self.tabs_backend_authority(),
            composite_tools_enabled: self.composite_tools_enabled(),
            grep_timeout_enforced: self.grep_timeout_enforced(),
        }
    }
}

/// Serializable snapshot of feature flags for frontend consumption
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FeatureFlagsSnapshot {
    pub editor_backend_authority: bool,
    pub tabs_backend_authority: bool,
    pub composite_tools_enabled: bool,
    pub grep_timeout_enforced: bool,
}
