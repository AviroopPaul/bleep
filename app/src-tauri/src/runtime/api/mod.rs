//! bleep's own agent loop over a provider API, for people who bring their own key.
//! One loop (`agent`), one adapter per wire format (`anthropic`, `openai`), and presets
//! that say which provider uses which format.
pub mod adapter;
pub mod agent;
pub mod anthropic;
pub mod openai;
pub mod presets;
pub mod prices;
pub mod types;

#[cfg(test)]
mod http_tests;

use crate::config::ApiConfig;
use crate::google::secrets::Secrets;
use crate::store::Store;
use adapter::ModelAdapter;
use agent::{ApiChoice, ApiRuntime, Core};
use presets::{preset, Wire};
use std::sync::Arc;

pub fn key_name(provider: &str) -> String {
    format!("api:{provider}:key")
}

/// The adapter for a provider. `base_url` empty means the preset's.
pub fn adapter(provider: &str, base_url: &str, key: &str) -> Result<Arc<dyn ModelAdapter>, String> {
    let p = preset(provider).ok_or_else(|| format!("Unknown provider: {provider}"))?;
    let url = if base_url.trim().is_empty() { p.base_url } else { base_url.trim() };
    if url.is_empty() {
        return Err("Enter the server's base URL.".into());
    }
    presets::check_base_url(url)?;
    Ok(match p.wire {
        Wire::Anthropic => Arc::new(anthropic::AnthropicAdapter::new(url, key)),
        Wire::OpenAi => Arc::new(openai::OpenAiAdapter::new(url, key, p.max_completion_tokens)),
    })
}

/// The API runtime for the saved settings. A bad setting still gives a runtime, whose
/// health says what to fix.
pub fn from_config(cfg: &ApiConfig, secrets: &dyn Secrets, store: Arc<Store>) -> ApiRuntime {
    let p = preset(&cfg.provider).or_else(|| preset("custom")).unwrap();
    let key = secrets.get(&key_name(p.id)).unwrap_or_default();
    let has_key = !p.needs_key || !key.is_empty();
    let choice = ApiChoice { provider: p.id.into(), label: p.label.into(), main: cfg.main.clone(), small: cfg.small.clone(), has_key, free: p.free };
    let adapter = adapter(p.id, &cfg.base_url, &key).unwrap_or_else(|e| Arc::new(Broken(e)));
    ApiRuntime::new(Core::new(choice, adapter, store))
}

/// An adapter for settings that cannot work (a bad base URL): every call says why.
struct Broken(String);

#[async_trait::async_trait]
impl ModelAdapter for Broken {
    async fn stream(&self, _: &types::ModelRequest) -> Result<types::ModelStream, types::ApiError> {
        Err(types::ApiError::Fatal(self.0.clone()))
    }
    async fn list_models(&self) -> Result<Vec<types::ModelInfo>, types::ApiError> {
        Err(types::ApiError::Fatal(self.0.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::secrets::MemorySecrets;
    use crate::runtime::AgentRuntime;

    #[tokio::test]
    async fn from_config_reads_the_key_for_its_provider() {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let s = MemorySecrets::default();
        let cfg = ApiConfig { provider: "anthropic".into(), base_url: String::new(), main: "a".into(), small: "b".into(), daily_cap_usd: 1.0 };
        assert!(!from_config(&cfg, &s, store.clone()).health().await.signed_in);
        s.set("api:anthropic:key", "sk").unwrap();
        let rt = from_config(&cfg, &s, store.clone());
        assert!(rt.health().await.ok());
        assert_eq!(rt.choice().label, "Anthropic");
        let ollama = ApiConfig { provider: "ollama".into(), ..cfg.clone() };
        let rt = from_config(&ollama, &s, store);
        assert!(rt.health().await.ok() && rt.choice().free, "a local server needs no key");
    }

    #[test]
    fn adapter_checks_the_url() {
        assert!(adapter("custom", "", "").err().unwrap().contains("base URL"));
        assert!(adapter("openai", "http://example.com/v1", "k").is_err());
        assert!(adapter("nope", "", "").is_err());
        assert!(adapter("openrouter", "", "k").is_ok());
    }
}
