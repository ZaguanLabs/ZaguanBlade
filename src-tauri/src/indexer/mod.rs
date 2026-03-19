pub mod builder;
pub mod cache;
pub mod handler;
pub mod manager;
pub mod minimal_index;
pub mod preview;
pub mod types;
pub mod watcher;

pub use builder::index_workspace;
pub use handler::handle_get_full_context;
pub use manager::IndexerManager;
pub use types::{CachedPreview, DirectoryTree, FileMetadata, ProjectIndex};
