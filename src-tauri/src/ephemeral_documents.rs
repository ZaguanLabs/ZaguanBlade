use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralDocument {
    pub id: String,
    pub content: String,
    pub suggested_name: String,
    pub created_at: DateTime<Utc>,
    pub modified: bool,
}

pub struct EphemeralDocumentStore {
    documents: Mutex<HashMap<String, EphemeralDocument>>,
}

impl EphemeralDocumentStore {
    pub fn new() -> Self {
        Self {
            documents: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(&self, content: String, suggested_name: String) -> String {
        let id = format!("ephemeral-{}", Utc::now().timestamp_millis());
        let doc = EphemeralDocument {
            id: id.clone(),
            content,
            suggested_name,
            created_at: Utc::now(),
            modified: false,
        };

        let mut docs = self.documents.lock().unwrap();
        docs.insert(id.clone(), doc);
        id
    }

    pub fn get(&self, id: &str) -> Option<EphemeralDocument> {
        let docs = self.documents.lock().unwrap();
        docs.get(id).cloned()
    }

    pub fn update_content(&self, id: &str, content: String) -> bool {
        let mut docs = self.documents.lock().unwrap();
        if let Some(doc) = docs.get_mut(id) {
            doc.content = content;
            doc.modified = true;
            true
        } else {
            false
        }
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut docs = self.documents.lock().unwrap();
        docs.remove(id).is_some()
    }

    pub fn list(&self) -> Vec<EphemeralDocument> {
        let docs = self.documents.lock().unwrap();
        docs.values().cloned().collect()
    }
}

impl Default for EphemeralDocumentStore {
    fn default() -> Self {
        Self::new()
    }
}
