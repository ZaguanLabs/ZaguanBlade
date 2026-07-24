//! M0.7 — Conversation load / page throughput baseline.
//!
//! `#[ignore]`-gated integration test that imports the long-conversation fixture
//! (text, reasoning, tool results, mentions and image references) and measures
//! both a full load and a paged load through `ConversationStore`.
//!
//! Run with:
//!   `cargo test --release --test bench_conversation_load -- --ignored --nocapture`
//!
//! Fixture selection:
//!   - `CONVERSATION_FIXTURE=/abs/path.json` overrides the path.
//!   - otherwise defaults to `{workspace}/benchmarks/corpora/long_chat.json`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use tempfile::TempDir;
use zblade_lib::conversation_store::{ConversationStore, StoredConversation};

fn fixture_path() -> PathBuf {
    if let Ok(raw) = std::env::var("CONVERSATION_FIXTURE") {
        let path = PathBuf::from(&raw);
        assert!(
            path.is_absolute(),
            "CONVERSATION_FIXTURE must be absolute, got {raw:?}"
        );
        assert!(
            path.is_file(),
            "CONVERSATION_FIXTURE must be a file, got {raw:?}"
        );
        return path;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cargo manifest has parent")
        .join("benchmarks/corpora/long_chat.json")
}

fn peak_rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                return rest.split_whitespace().next()?.parse::<u64>().ok();
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[test]
#[ignore = "conversation load bench; run explicitly with --ignored --nocapture"]
fn bench_conversation_load() {
    let fixture = fixture_path();
    assert!(
        fixture.is_file(),
        "fixture not found: {}",
        fixture.display()
    );

    let raw = std::fs::read_to_string(&fixture).expect("read fixture");
    let fixture_bytes = raw.len() as u64;
    let conv: StoredConversation = serde_json::from_str(&raw).expect("parse fixture");
    let message_count = conv.messages.len();

    let tmp = TempDir::new().expect("create conversation store tmp dir");
    let mut store = ConversationStore::new(tmp.path().to_path_buf()).expect("open store");

    let save_start = Instant::now();
    store.save_conversation(&conv).expect("save fixture");
    let save_us = save_start.elapsed().as_micros() as u64;

    let full_start = Instant::now();
    let loaded = store
        .load_conversation(&conv.metadata.id)
        .expect("full load");
    let full_load_us = full_start.elapsed().as_micros() as u64;
    assert_eq!(
        loaded.messages.len(),
        message_count,
        "full load should restore all messages"
    );

    let page_size = 50usize;
    let page_start = Instant::now();
    let page = store
        .load_message_page(&conv.metadata.id, 0, page_size)
        .expect("page load");
    let page_load_us = page_start.elapsed().as_micros() as u64;
    assert_eq!(
        page.total, message_count,
        "page load total should equal fixture size"
    );
    assert_eq!(
        page.messages.len(),
        page_size.min(message_count),
        "page load returned expected count"
    );

    let middle_offset = message_count.saturating_sub(page_size) / 2;
    let middle_start = Instant::now();
    let middle_page = store
        .load_message_page(&conv.metadata.id, middle_offset, page_size)
        .expect("middle page load");
    let middle_page_load_us = middle_start.elapsed().as_micros() as u64;
    assert!(
        !middle_page.messages.is_empty(),
        "middle page should not be empty"
    );

    let report = serde_json::json!({
        "fixture": fixture.to_string_lossy(),
        "fixture_bytes": fixture_bytes,
        "message_count": message_count,
        "save_us": save_us,
        "save_ms": (save_us as f64) / 1000.0,
        "full_load_us": full_load_us,
        "full_load_ms": (full_load_us as f64) / 1000.0,
        "page_load_us": page_load_us,
        "page_load_ms": (page_load_us as f64) / 1000.0,
        "middle_page_load_us": middle_page_load_us,
        "middle_page_load_ms": (middle_page_load_us as f64) / 1000.0,
        "page_size": page_size,
        "peak_rss_kb": peak_rss_kb(),
    });

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    println!("BENCH_JSON {}", serde_json::to_string(&report).unwrap());

    assert!(save_us > 0, "save should take measurable time");
    assert!(full_load_us > 0, "full load should take measurable time");
    assert!(page_load_us > 0, "page load should take measurable time");
}
