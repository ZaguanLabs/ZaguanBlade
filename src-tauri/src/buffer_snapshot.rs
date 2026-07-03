use crate::tree_sitter::{Language, Symbol};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSnapshotSource {
    Disk,
    Live,
}

#[derive(Debug, Clone)]
pub struct BufferSnapshot {
    version: Option<i32>,
    hash: String,
    language: Option<Language>,
    source: BufferSnapshotSource,
    content: Arc<str>,
    line_starts: Arc<Vec<usize>>,
}

impl BufferSnapshot {
    pub fn new(
        path: impl Into<String>,
        version: Option<i32>,
        content: impl Into<String>,
        source: BufferSnapshotSource,
    ) -> Self {
        let path = path.into();
        let content = content.into();
        let language = Language::from_path(&path);
        let hash = compute_hash(&content);
        let line_starts = compute_line_starts(&content);
        Self {
            version,
            hash,
            language,
            source,
            content: Arc::<str>::from(content),
            line_starts: Arc::new(line_starts),
        }
    }

    pub fn version(&self) -> Option<i32> {
        self.version
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn language(&self) -> Option<Language> {
        self.language
    }

    pub fn is_live(&self) -> bool {
        self.source == BufferSnapshotSource::Live
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn to_string(&self) -> String {
        self.content.to_string()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len().max(1)
    }

    pub fn excerpt_around_line(&self, line: u32, padding: usize) -> String {
        let line = line as usize;
        let start = line.saturating_sub(padding);
        let end = (line + padding + 1).min(self.line_count());
        self.slice_lines(start, end)
    }

    pub fn slice_lines(&self, start_line: usize, end_line_exclusive: usize) -> String {
        if start_line >= end_line_exclusive || start_line >= self.line_count() {
            return String::new();
        }
        let end_line_exclusive = end_line_exclusive.min(self.line_count());
        let start_byte = self.line_starts[start_line];
        let end_byte = if end_line_exclusive < self.line_starts.len() {
            self.line_starts[end_line_exclusive]
        } else {
            self.content.len()
        };
        self.content[start_byte..end_byte].to_string()
    }

    pub fn symbol_byte_range(&self, symbol: &Symbol) -> Result<(usize, usize), String> {
        self.byte_range(
            symbol.range.start.line as usize,
            symbol.range.start.character as usize,
            symbol.range.end.line as usize,
            symbol.range.end.character as usize,
        )
    }

    pub fn line_byte_range(
        &self,
        start_line_1based: u32,
        end_line_1based: u32,
    ) -> Result<(usize, usize), String> {
        let start_idx = start_line_1based.saturating_sub(1) as usize;
        let end_idx = end_line_1based as usize;
        if start_idx >= self.line_count() {
            return Err("Start line out of bounds".to_string());
        }
        let start_byte = self.line_starts[start_idx];
        let bounded_end = end_idx.min(self.line_count());
        let end_byte = if bounded_end < self.line_starts.len() {
            self.line_starts[bounded_end]
        } else {
            self.content.len()
        };
        Ok((start_byte, end_byte))
    }

    pub fn byte_range(
        &self,
        start_line: usize,
        start_character: usize,
        end_line: usize,
        end_character: usize,
    ) -> Result<(usize, usize), String> {
        if start_line >= self.line_count() {
            return Err("Symbol start line out of bounds".to_string());
        }
        if end_line >= self.line_count() {
            return Err("Symbol end line out of bounds".to_string());
        }
        let start_byte = self.line_starts[start_line] + start_character;
        let end_byte = self.line_starts[end_line] + end_character;
        if start_byte > self.content.len() || end_byte > self.content.len() || start_byte > end_byte
        {
            return Err("Invalid byte range".to_string());
        }
        Ok((start_byte, end_byte))
    }
}

#[derive(Default)]
pub struct BufferSnapshotStore {
    snapshots: RwLock<HashMap<String, Arc<BufferSnapshot>>>,
}

impl BufferSnapshotStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, path: &str) -> Option<Arc<BufferSnapshot>> {
        self.snapshots.read().unwrap().get(path).cloned()
    }

    pub fn contains_live(&self, path: &str) -> bool {
        self.get(path)
            .map(|snapshot| snapshot.is_live())
            .unwrap_or(false)
    }

    pub fn upsert_live(
        &self,
        path: &str,
        version: Option<i32>,
        content: &str,
    ) -> Arc<BufferSnapshot> {
        self.upsert(path, version, content, BufferSnapshotSource::Live)
    }

    pub fn upsert_disk(&self, path: &str, content: &str) -> Arc<BufferSnapshot> {
        self.upsert(path, None, content, BufferSnapshotSource::Disk)
    }

    pub fn remove(&self, path: &str) -> Option<Arc<BufferSnapshot>> {
        self.snapshots.write().unwrap().remove(path)
    }

    fn upsert(
        &self,
        path: &str,
        version: Option<i32>,
        content: &str,
        source: BufferSnapshotSource,
    ) -> Arc<BufferSnapshot> {
        let snapshot = Arc::new(BufferSnapshot::new(
            path.to_string(),
            version,
            content,
            source,
        ));
        self.snapshots
            .write()
            .unwrap()
            .insert(path.to_string(), snapshot.clone());
        snapshot
    }
}

fn compute_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, byte) in content.bytes().enumerate() {
        if byte == b'\n' && idx + 1 < content.len() {
            starts.push(idx + 1);
        }
    }
    starts
}

fn compute_hash(content: &str) -> String {
    // M6 CL1 — must match service.rs::compute_hash (both feed the persisted
    // file_hash). Version-stable across toolchains (was SipHash `DefaultHasher`).
    crate::stable_hash::stable_hash_hex(content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::{Position, Range, Symbol, SymbolType};

    #[test]
    fn snapshot_line_and_byte_ranges_are_stable() {
        let snapshot = BufferSnapshot::new(
            "src/main.rs",
            Some(3),
            "fn main() {\n    println!(\"hi\");\n}\n",
            BufferSnapshotSource::Live,
        );
        assert_eq!(
            snapshot.excerpt_around_line(1, 1),
            "fn main() {\n    println!(\"hi\");\n}\n"
        );
        assert_eq!(snapshot.line_byte_range(2, 2).unwrap(), (12, 32));
    }

    #[test]
    fn snapshot_symbol_range_uses_immutable_content() {
        let snapshot = BufferSnapshot::new(
            "src/lib.rs",
            Some(7),
            "pub fn hello() {\n    world();\n}\n",
            BufferSnapshotSource::Live,
        );
        let symbol = Symbol {
            id: "1".to_string(),
            name: "hello".to_string(),
            qualified_name: "hello".to_string(),
            symbol_type: SymbolType::Function,
            file_path: "src/lib.rs".to_string(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 1,
                },
            },
            byte_offset: 0,
            byte_length: 34,
            parent_id: None,
            docstring: None,
            signature: None,
            content_hash: String::new(),
        };
        let (start, end) = snapshot.symbol_byte_range(&symbol).unwrap();
        assert_eq!(
            &snapshot.content()[start..end],
            "pub fn hello() {\n    world();\n}"
        );
    }

    #[test]
    fn store_tracks_live_snapshots_by_logical_path() {
        let store = BufferSnapshotStore::new();
        store.upsert_live("src/main.rs", Some(5), "fn main() {}\n");
        assert!(store.contains_live("src/main.rs"));
        assert_eq!(store.get("src/main.rs").unwrap().version(), Some(5));
        store.remove("src/main.rs");
        assert!(store.get("src/main.rs").is_none());
    }
}
