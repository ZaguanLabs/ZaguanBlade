pub mod types;
pub mod builder;
pub mod watcher;
pub mod preview;
pub mod handler;
pub mod cache;
pub mod manager;

pub use types::{ProjectIndex, FileMetadata, CachedPreview, DirectoryTree};
pub use builder::index_workspace;
pub use handler::handle_get_full_context;
pub use manager::IndexerManager;
