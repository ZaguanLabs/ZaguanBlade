use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Idempotency cache entry
#[derive(Clone)]
struct CacheEntry {
    intent_id: Uuid,
    result: IdempotencyResult,
    expires_at: Instant,
}

/// Result of an idempotent operation
#[derive(Clone)]
pub enum IdempotencyResult {
    Success,
    Failed { error: String },
}

/// Idempotency cache with TTL
pub struct IdempotencyCache {
    cache: Mutex<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl IdempotencyCache {
    /// Create a new idempotency cache with the specified TTL
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Check if an idempotency key has been processed
    /// Returns Some(intent_id) if found and not expired, None otherwise
    pub fn check(&self, key: &str) -> Option<(Uuid, IdempotencyResult)> {
        let mut cache = self.cache.lock().unwrap();
        
        // Clean up expired entries while we're here
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        
        // Check if key exists and is not expired
        if let Some(entry) = cache.get(key) {
            if entry.expires_at > now {
                return Some((entry.intent_id, entry.result.clone()));
            }
        }
        
        None
    }

    /// Store a successful result for an idempotency key
    pub fn store_success(&self, key: String, intent_id: Uuid) {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(
            key,
            CacheEntry {
                intent_id,
                result: IdempotencyResult::Success,
                expires_at: Instant::now() + self.ttl,
            },
        );
    }

    /// Store a failed result for an idempotency key
    pub fn store_failure(&self, key: String, intent_id: Uuid, error: String) {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(
            key,
            CacheEntry {
                intent_id,
                result: IdempotencyResult::Failed { error },
                expires_at: Instant::now() + self.ttl,
            },
        );
    }

    /// Clear all entries (useful for testing)
    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    /// Get the number of cached entries (useful for monitoring)
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.len()
    }
}

impl Default for IdempotencyCache {
    fn default() -> Self {
        // Default TTL: 24 hours
        Self::new(Duration::from_secs(24 * 60 * 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_idempotency_cache_basic() {
        let cache = IdempotencyCache::new(Duration::from_secs(1));
        let key = "test-key".to_string();
        let intent_id = Uuid::new_v4();

        // Initially, key should not exist
        assert!(cache.check(&key).is_none());

        // Store success
        cache.store_success(key.clone(), intent_id);

        // Should now exist
        let result = cache.check(&key);
        assert!(result.is_some());
        let (cached_id, cached_result) = result.unwrap();
        assert_eq!(cached_id, intent_id);
        assert!(matches!(cached_result, IdempotencyResult::Success));
    }

    #[test]
    fn test_idempotency_cache_expiry() {
        let cache = IdempotencyCache::new(Duration::from_millis(100));
        let key = "test-key".to_string();
        let intent_id = Uuid::new_v4();

        cache.store_success(key.clone(), intent_id);
        assert!(cache.check(&key).is_some());

        // Wait for expiry
        thread::sleep(Duration::from_millis(150));

        // Should be expired
        assert!(cache.check(&key).is_none());
    }

    #[test]
    fn test_idempotency_cache_failure() {
        let cache = IdempotencyCache::new(Duration::from_secs(1));
        let key = "test-key".to_string();
        let intent_id = Uuid::new_v4();
        let error = "Test error".to_string();

        cache.store_failure(key.clone(), intent_id, error.clone());

        let result = cache.check(&key);
        assert!(result.is_some());
        let (_, cached_result) = result.unwrap();
        match cached_result {
            IdempotencyResult::Failed { error: e } => assert_eq!(e, error),
            _ => panic!("Expected Failed result"),
        }
    }
}
