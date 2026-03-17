use crate::config::ApiConfig;
use crate::models::registry::ModelInfo;

pub async fn list_all_models(config: &ApiConfig) -> Vec<ModelInfo> {
    let mut models = if config.api_key.trim().is_empty() {
        Vec::new()
    } else {
        crate::models::registry::get_models(&config.blade_url, &config.api_key).await
    };

    if config.ollama_enabled {
        let mut ollama_models = crate::models::ollama::get_models(
            &config.ollama_url,
            config.ollama_cloud_enabled,
            &config.ollama_cloud_api_key,
        )
        .await;
        models.append(&mut ollama_models);
    }

    if config.openai_compat_enabled {
        let mut openai_compat_models =
            crate::models::openai_compat::get_models(&config.openai_compat_url).await;
        models.append(&mut openai_compat_models);
    }

    models
}

pub fn resolve_model_index(models: &[ModelInfo], model_id: &str) -> Option<usize> {
    models
        .iter()
        .position(|m| m.id == model_id)
        .or_else(|| {
            models
                .iter()
                .position(|m| m.api_id.as_deref() == Some(model_id))
        })
        .or_else(|| {
            let id_lower = model_id.to_lowercase();
            models
                .iter()
                .position(|m| m.id.to_lowercase() == id_lower)
                .or_else(|| {
                    models.iter().position(|m| {
                        m.api_id
                            .as_ref()
                            .map(|s| s.to_lowercase())
                            .as_deref()
                            == Some(&id_lower)
                    })
                })
        })
}
