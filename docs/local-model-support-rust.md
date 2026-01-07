# Local Model Support - Native Rust Implementation

## Architecture

**Key Design:** Local model connections (Ollama, OpenAI-compatible) are built **natively in zblade (Rust/Tauri)**, not routed through zcoderd.

```
┌─────────────────────────────────────────────────────┐
│              zblade (Rust/Tauri)                    │
│  ┌───────────────────────────────────────────────┐  │
│  │         AI Provider Manager (Rust)            │  │
│  └───────────────────────────────────────────────┘  │
│           │              │              │            │
│           ▼              ▼              ▼            │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐  │
│  │   Ollama    │ │  OpenAI API │ │  Zaguán AI  │  │
│  │   Client    │ │   Client    │ │   Client    │  │
│  │   (Rust)    │ │   (Rust)    │ │   (Rust)    │  │
│  └─────────────┘ └─────────────┘ └─────────────┘  │
│        │              │                  │          │
│        │ Direct       │ Direct           │ Via      │
│        │ HTTP         │ HTTP             │ zcoderd  │
│        ▼              ▼                  ▼          │
└────────┼──────────────┼──────────────────┼──────────┘
         │              │                  │
         ▼              ▼                  ▼
  ┌──────────┐   ┌──────────┐      ┌──────────┐
  │  Ollama  │   │LM Studio │      │ zcoderd  │
  │localhost │   │localhost │      │  Proxy   │
  │  :11434  │   │  :1234   │      │  Cloud   │
  └──────────┘   └──────────┘      └──────────┘
```

**Benefits:**
- ✅ Lower latency (no proxy hop)
- ✅ Works offline (no zcoderd needed for local)
- ✅ Better performance (native Rust HTTP)
- ✅ User privacy (local stays local)
- ✅ Simpler architecture

---

## Rust Implementation

### File Structure

```
src-tauri/src/
├── ai_providers/
│   ├── mod.rs              # Trait definition & manager
│   ├── ollama.rs           # Ollama client
│   ├── openai_compatible.rs # OpenAI-compatible client
│   └── zaguan.rs           # Zaguán AI client (via zcoderd)
├── lib.rs                  # Register Tauri commands
└── main.rs
```

### 1. Provider Trait

**File: `src-tauri/src/ai_providers/mod.rs`**

```rust
pub mod ollama;
pub mod openai_compatible;
pub mod zaguan;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::error::Error;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_size: usize,
    pub supports_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
    pub stream: bool,
}

#[async_trait]
pub trait AIProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn list_models(&self) -> Result<Vec<Model>, Box<dyn Error>>;
    async fn stream_completion(
        &self,
        request: CompletionRequest,
    ) -> Result<mpsc::Receiver<String>, Box<dyn Error>>;
    async fn is_available(&self) -> bool;
    fn default_model(&self) -> &str;
}

pub struct ProviderManager {
    providers: std::collections::HashMap<String, Box<dyn AIProvider>>,
}

impl ProviderManager {
    pub fn new() -> Self {
        Self {
            providers: std::collections::HashMap::new(),
        }
    }
    
    pub fn register(&mut self, provider: Box<dyn AIProvider>) {
        self.providers.insert(provider.name().to_string(), provider);
    }
    
    pub fn get(&self, name: &str) -> Option<&Box<dyn AIProvider>> {
        self.providers.get(name)
    }
    
    pub async fn list_all_models(&self) -> Vec<Model> {
        let mut all_models = Vec::new();
        for provider in self.providers.values() {
            if let Ok(models) = provider.list_models().await {
                all_models.extend(models);
            }
        }
        all_models
    }
}
```

### 2. Ollama Client

**File: `src-tauri/src/ai_providers/ollama.rs`**

```rust
use super::*;
use reqwest::Client;
use serde_json::json;

pub struct OllamaProvider {
    base_url: String,
    client: Client,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
            client: Client::new(),
        }
    }
}

#[async_trait]
impl AIProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }
    
    async fn list_models(&self) -> Result<Vec<Model>, Box<dyn Error>> {
        #[derive(Deserialize)]
        struct OllamaModel {
            name: String,
            size: i64,
        }
        
        #[derive(Deserialize)]
        struct OllamaResponse {
            models: Vec<OllamaModel>,
        }
        
        let url = format!("{}/api/tags", self.base_url);
        let response = self.client.get(&url).send().await?;
        let data: OllamaResponse = response.json().await?;
        
        Ok(data.models.into_iter().map(|m| Model {
            id: m.name.clone(),
            name: m.name,
            provider: "ollama".to_string(),
            context_size: 4096,
            supports_tools: false,
        }).collect())
    }
    
    async fn stream_completion(
        &self,
        request: CompletionRequest,
    ) -> Result<mpsc::Receiver<String>, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel(100);
        
        let url = format!("{}/api/chat", self.base_url);
        let body = json!({
            "model": request.model,
            "messages": request.messages,
            "stream": true,
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
            }
        });
        
        let client = self.client.clone();
        tokio::spawn(async move {
            match client.post(&url).json(&body).send().await {
                Ok(response) => {
                    let mut stream = response.bytes_stream();
                    use futures::StreamExt;
                    
                    while let Some(chunk) = stream.next().await {
                        if let Ok(bytes) = chunk {
                            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                                for line in text.lines() {
                                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                                        if let Some(content) = json["message"]["content"].as_str() {
                                            let _ = tx.send(content.to_string()).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Ollama request failed: {}", e);
                }
            }
        });
        
        Ok(rx)
    }
    
    async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client.get(&url).send().await.is_ok()
    }
    
    fn default_model(&self) -> &str {
        "codellama:7b"
    }
}
```

### 3. OpenAI-Compatible Client

**File: `src-tauri/src/ai_providers/openai_compatible.rs`**

```rust
use super::*;
use reqwest::Client;
use serde_json::json;

pub struct OpenAICompatibleProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    client: Client,
}

impl OpenAICompatibleProvider {
    pub fn new(name: String, base_url: String, api_key: Option<String>) -> Self {
        Self {
            name,
            base_url,
            api_key,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl AIProvider for OpenAICompatibleProvider {
    fn name(&self) -> &str {
        &self.name
    }
    
    async fn list_models(&self) -> Result<Vec<Model>, Box<dyn Error>> {
        #[derive(Deserialize)]
        struct ModelData {
            id: String,
        }
        
        #[derive(Deserialize)]
        struct ModelsResponse {
            data: Vec<ModelData>,
        }
        
        let url = format!("{}/models", self.base_url);
        let mut request = self.client.get(&url);
        
        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }
        
        let response = request.send().await?;
        let data: ModelsResponse = response.json().await?;
        
        Ok(data.data.into_iter().map(|m| Model {
            id: m.id.clone(),
            name: m.id,
            provider: self.name.clone(),
            context_size: 4096,
            supports_tools: true,
        }).collect())
    }
    
    async fn stream_completion(
        &self,
        request: CompletionRequest,
    ) -> Result<mpsc::Receiver<String>, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel(100);
        
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": request.model,
            "messages": request.messages,
            "stream": true,
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens,
        });
        
        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        
        let client_clone = req;
        tokio::spawn(async move {
            match client_clone.send().await {
                Ok(response) => {
                    let mut stream = response.bytes_stream();
                    use futures::StreamExt;
                    
                    while let Some(chunk) = stream.next().await {
                        if let Ok(bytes) = chunk {
                            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                                for line in text.lines() {
                                    if line.starts_with("data: ") {
                                        let json_str = &line[6..];
                                        if json_str == "[DONE]" {
                                            break;
                                        }
                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                                            if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                                let _ = tx.send(content.to_string()).await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("OpenAI-compatible request failed: {}", e);
                }
            }
        });
        
        Ok(rx)
    }
    
    async fn is_available(&self) -> bool {
        let url = format!("{}/models", self.base_url);
        let mut request = self.client.get(&url);
        
        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }
        
        request.send().await.is_ok()
    }
    
    fn default_model(&self) -> &str {
        "local-model"
    }
}
```

### 4. Tauri Commands

**File: `src-tauri/src/lib.rs` (UPDATE)**

```rust
use ai_providers::{ProviderManager, Model, ChatMessage, CompletionRequest};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    // ... existing fields
    pub provider_manager: Mutex<ProviderManager>,
}

#[tauri::command]
async fn list_ai_providers(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let manager = state.provider_manager.lock().unwrap();
    Ok(manager.providers.keys().cloned().collect())
}

#[tauri::command]
async fn list_models(
    provider_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<Model>, String> {
    let manager = state.provider_manager.lock().unwrap();
    
    if let Some(name) = provider_name {
        if let Some(provider) = manager.get(&name) {
            provider.list_models().await
                .map_err(|e| e.to_string())
        } else {
            Err(format!("Provider {} not found", name))
        }
    } else {
        Ok(manager.list_all_models().await)
    }
}

#[tauri::command]
async fn stream_chat(
    provider_name: String,
    model: String,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), String> {
    let manager = state.provider_manager.lock().unwrap();
    
    let provider = manager.get(&provider_name)
        .ok_or_else(|| format!("Provider {} not found", provider_name))?;
    
    let request = CompletionRequest {
        model,
        messages,
        temperature,
        max_tokens: None,
        stream: true,
    };
    
    let mut rx = provider.stream_completion(request).await
        .map_err(|e| e.to_string())?;
    
    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let _ = window.emit("chat-chunk", chunk);
        }
        let _ = window.emit("chat-done", ());
    });
    
    Ok(())
}

#[tauri::command]
async fn test_provider_connection(
    provider_name: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let manager = state.provider_manager.lock().unwrap();
    
    if let Some(provider) = manager.get(&provider_name) {
        Ok(provider.is_available().await)
    } else {
        Err(format!("Provider {} not found", provider_name))
    }
}

// Initialize providers on app startup
pub fn init_providers() -> ProviderManager {
    let mut manager = ProviderManager::new();
    
    // Register Ollama (auto-detect)
    manager.register(Box::new(ai_providers::ollama::OllamaProvider::new(None)));
    
    // Register custom providers from config
    // TODO: Load from config file
    
    manager
}
```

### 5. Dependencies

**File: `src-tauri/Cargo.toml` (UPDATE)**

```toml
[dependencies]
# Existing dependencies...

# HTTP client
reqwest = { version = "0.11", features = ["json", "stream"] }

# Async
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

---

## Frontend Integration

**File: `src/hooks/useAIProviders.ts` (NEW)**

```typescript
import { invoke } from '@tauri-apps/api';
import { listen } from '@tauri-apps/api/event';
import { useState, useEffect } from 'react';

interface Model {
    id: string;
    name: string;
    provider: string;
    context_size: number;
    supports_tools: boolean;
}

export const useAIProviders = () => {
    const [providers, setProviders] = useState<string[]>([]);
    const [models, setModels] = useState<Model[]>([]);
    const [selectedProvider, setSelectedProvider] = useState<string>('zaguan');
    const [selectedModel, setSelectedModel] = useState<string>('');
    
    useEffect(() => {
        loadProviders();
    }, []);
    
    const loadProviders = async () => {
        try {
            const providerList = await invoke<string[]>('list_ai_providers');
            setProviders(providerList);
            
            const allModels = await invoke<Model[]>('list_models');
            setModels(allModels);
        } catch (e) {
            console.error('Failed to load providers:', e);
        }
    };
    
    const testConnection = async (providerName: string) => {
        try {
            const isAvailable = await invoke<boolean>('test_provider_connection', {
                providerName,
            });
            return isAvailable;
        } catch (e) {
            console.error('Connection test failed:', e);
            return false;
        }
    };
    
    const streamChat = async (
        messages: Array<{role: string; content: string}>,
        onChunk: (chunk: string) => void,
        onDone: () => void,
    ) => {
        const unlisten1 = await listen<string>('chat-chunk', (event) => {
            onChunk(event.payload);
        });
        
        const unlisten2 = await listen('chat-done', () => {
            onDone();
            unlisten1();
            unlisten2();
        });
        
        await invoke('stream_chat', {
            providerName: selectedProvider,
            model: selectedModel,
            messages,
            temperature: 0.7,
        });
    };
    
    return {
        providers,
        models,
        selectedProvider,
        selectedModel,
        setSelectedProvider,
        setSelectedModel,
        testConnection,
        streamChat,
        loadProviders,
    };
};
```

---

## Configuration

**File: `src-tauri/tauri.conf.json` (UPDATE)**

```json
{
  "tauri": {
    "allowlist": {
      "http": {
        "all": true,
        "request": true,
        "scope": [
          "http://localhost:*",
          "http://127.0.0.1:*",
          "https://api.zaguan.ai/*"
        ]
      }
    }
  }
}
```

---

## Implementation Timeline

### Week 1: Core Infrastructure
- [ ] Create AI provider trait and manager
- [ ] Implement Ollama client
- [ ] Implement OpenAI-compatible client
- [ ] Add Tauri commands
- [ ] Test basic connectivity

### Week 2: Frontend Integration
- [ ] Create provider selection UI
- [ ] Implement model selector
- [ ] Add connection testing
- [ ] Update chat hook to use providers
- [ ] Test streaming

### Week 3: Polish & Testing
- [ ] Settings panel for providers
- [ ] Auto-detection of local providers
- [ ] Error handling
- [ ] Documentation
- [ ] End-to-end testing

**Total: 3 weeks to production-ready**

---

## Benefits of Rust Implementation

1. **Performance**: Native HTTP, no serialization overhead
2. **Safety**: Rust's type system prevents common bugs
3. **Async**: Tokio for efficient streaming
4. **Offline**: Works without zcoderd for local models
5. **Privacy**: Local connections stay local
6. **Simple**: No proxy layer, direct HTTP

---

**Status:** Ready for implementation  
**Priority:** High  
**Complexity:** Medium (3 weeks)  
**ROI:** Very High (competitive differentiator)
