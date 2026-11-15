//! Tauri commands for Settings > Model (Phase 9): which engine runs bleep, which provider and
//! models, the key, and today's spend.
use crate::config::{ApiConfig, Config, Tiers};
use crate::google::secrets::{KeychainSecrets, Secrets};
use crate::runtime::api::presets::{preset, Preset, PRESETS};
use crate::runtime::api::types::{ApiError, ModelEvent, ModelInfo, ModelRequest, Msg, ToolChoice};
use crate::runtime::api::{adapter, key_name};
use crate::store::api::{ApiUsage, Spend};
use crate::AppState;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Debug)]
pub struct ModelSettings {
    pub runtime: String,
    pub tiers: Tiers,
    pub api: ApiConfig,
    pub presets: &'static [Preset],
    /// Providers with a saved key. Never the key itself.
    pub keys: Vec<&'static str>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ModelSave {
    pub runtime: String,
    pub tiers: Tiers,
    pub api: ApiConfig,
    /// A new key for `api.provider`. None keeps the saved one.
    pub key: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TestResult {
    pub ok: bool,
    pub ms: u64,
    pub cost: Option<f64>,
    pub message: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SpendView {
    pub runtime: String,
    pub spend: Spend,
    pub cap: f64,
    pub over: bool,
}

pub fn saved_keys(secrets: &dyn Secrets) -> Vec<&'static str> {
    PRESETS.iter().filter(|p| p.needs_key && secrets.get(&key_name(p.id)).is_some_and(|k| !k.is_empty())).map(|p| p.id).collect()
}

/// Checks a save and gives the config to write. The key is stored by the caller.
pub fn apply(cfg: &Config, req: &ModelSave, has_key: bool) -> Result<Config, String> {
    let mut out = cfg.clone();
    match req.runtime.as_str() {
        "claude" => {
            let ok = |t: &str| ["opus", "sonnet", "haiku"].contains(&t) || t.starts_with("claude-");
            if !ok(&req.tiers.main) || !ok(&req.tiers.small) {
                return Err("Pick opus, sonnet or haiku for Claude Code.".into());
            }
            out.tiers = req.tiers.clone();
        }
        "api" => {
            let p = preset(&req.api.provider).ok_or("Pick a provider.")?;
            let a = &req.api;
            if a.main.trim().is_empty() || a.small.trim().is_empty() {
                return Err("Pick a main model and a small model.".into());
            }
            if p.needs_key && !has_key {
                return Err(format!("Add your {} key.", p.label));
            }
            adapter(p.id, &a.base_url, "")?;
            if !(a.daily_cap_usd >= 0.0 && a.daily_cap_usd <= 1000.0) {
                return Err("The daily cap must be between $0 and $1000.".into());
            }
            let base = if a.base_url.trim() == p.base_url { String::new() } else { a.base_url.trim().trim_end_matches('/').to_string() };
            out.api = ApiConfig { provider: p.id.into(), base_url: if base.is_empty() { p.base_url.into() } else { base }, main: a.main.trim().into(), small: a.small.trim().into(), daily_cap_usd: a.daily_cap_usd };
        }
        _ => return Err("Pick Claude Code or an API key.".into()),
    }
    out.runtime = req.runtime.clone();
    Ok(out)
}

fn key_for(provider: &str, given: Option<String>) -> String {
    given.filter(|k| !k.trim().is_empty()).map(|k| k.trim().to_string()).unwrap_or_else(|| KeychainSecrets.get(&key_name(provider)).unwrap_or_default())
}

fn api_text(e: ApiError) -> String {
    match e {
        ApiError::RateLimited { .. } => "The provider's rate limit was hit. Try again in a minute.".into(),
        e => e.to_string(),
    }
}

#[tauri::command]
pub fn model_settings_get(state: tauri::State<'_, AppState>) -> ModelSettings {
    let c = state.config.lock().unwrap().clone();
    ModelSettings { runtime: c.runtime, tiers: c.tiers, api: c.api, presets: PRESETS, keys: saved_keys(&KeychainSecrets) }
}

/// The provider's models. Uses `key` if given, else the saved key.
#[tauri::command]
pub async fn model_list(provider: String, base_url: String, key: Option<String>) -> Result<Vec<ModelInfo>, String> {
    let a = adapter(&provider, &base_url, &key_for(&provider, key))?;
    a.list_models().await.map_err(api_text)
}

/// One tiny call to `model`, to check the key and the model before saving.
#[tauri::command]
pub async fn model_test(state: tauri::State<'_, AppState>, provider: String, base_url: String, key: Option<String>, model: String) -> Result<TestResult, String> {
    let a = adapter(&provider, &base_url, &key_for(&provider, key))?;
    let req = ModelRequest { model: model.clone(), system: String::new(), msgs: vec![Msg::user_text("Reply with the single word OK.")], tools: vec![], choice: ToolChoice::Auto, max_tokens: 16 };
    let start = Instant::now();
    let run = async {
        let mut s = a.stream(&req).await?;
        let (mut text, mut tokens) = (String::new(), None);
        while let Some(ev) = s.next().await {
            match ev? {
                ModelEvent::TextDelta(t) => text.push_str(&t),
                ModelEvent::Usage(t) => tokens = Some(t),
                _ => {}
            }
        }
        Ok::<_, ApiError>((text, tokens))
    };
    let out = tokio::time::timeout(std::time::Duration::from_secs(60), run).await;
    let ms = start.elapsed().as_millis() as u64;
    Ok(match out {
        Ok(Ok((_, tokens))) => {
            let price = preset(&provider).filter(|p| p.free).map(|_| Default::default()).or_else(|| crate::runtime::api::prices::builtin(&model));
            let cost = tokens.and_then(|t| price.map(|p: crate::runtime::api::types::Price| p.cost(&t)));
            if let Some(t) = tokens {
                let day = chrono::Local::now().date_naive().to_string();
                let _ = state.store.log_api_usage(&ApiUsage { day: &day, provider: &provider, model: &model, purpose: "test", tokens_in: t.input, cache_read: t.cache_read, cache_write: t.cache_write, tokens_out: t.output, cost });
            }
            TestResult { ok: true, ms, cost, message: format!("{model} answered in {:.1} s.", ms as f64 / 1000.0) }
        }
        Ok(Err(e)) => TestResult { ok: false, ms, cost: None, message: api_text(e) },
        Err(_) => TestResult { ok: false, ms, cost: None, message: "No answer within 60 seconds.".into() },
    })
}

#[tauri::command]
pub async fn model_save(app: AppHandle, state: tauri::State<'_, AppState>, req: ModelSave) -> Result<(), String> {
    let new_key = req.key.clone().filter(|k| !k.trim().is_empty());
    let provider = req.api.provider.clone();
    let has_key = new_key.is_some() || saved_keys(&KeychainSecrets).contains(&provider.as_str());
    let cfg = {
        let current = state.config.lock().unwrap().clone();
        apply(&current, &req, has_key)?
    };
    if req.runtime == "api" {
        if let Some(k) = &new_key {
            KeychainSecrets.set(&key_name(&provider), k.trim()).map_err(|e| format!("The key could not be saved in the Keychain: {e}"))?;
        }
    }
    {
        let mut c = state.config.lock().unwrap();
        *c = cfg.clone();
        c.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    }
    state.runtime.set((state.make_runtime)(&cfg));
    let _ = app.emit("runtime://changed", ());
    let chat = state.chat.clone();
    tauri::async_runtime::spawn(async move {
        if chat.runtime().health().await.ok() {
            if let Err(e) = chat.ensure_session().await {
                eprintln!("bleep: could not start the chat session: {e}");
            }
        }
    });
    Ok(())
}

/// Deletes a provider's saved key. The current runtime keeps working until the next save.
#[tauri::command]
pub fn model_forget_key(app: AppHandle, provider: String) -> Result<(), String> {
    KeychainSecrets.delete(&key_name(&provider)).map_err(|e| e.to_string())?;
    let st = app.state::<AppState>();
    let cfg = st.config.lock().unwrap().clone();
    if cfg.runtime == "api" && cfg.api.provider == provider {
        st.runtime.set((st.make_runtime)(&cfg));
        let _ = app.emit("runtime://changed", ());
    }
    Ok(())
}

#[tauri::command]
pub fn spend_today(state: tauri::State<'_, AppState>) -> Result<SpendView, String> {
    let c = state.config.lock().unwrap().clone();
    let spend = state.store.spend_on(&chrono::Local::now().date_naive().to_string()).map_err(|e| e.to_string())?;
    Ok(SpendView { runtime: c.runtime, spend, cap: c.api.daily_cap_usd, over: state.budget.over() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::secrets::MemorySecrets;

    fn save(runtime: &str, provider: &str, main: &str) -> ModelSave {
        ModelSave { runtime: runtime.into(), tiers: Tiers { main: "opus".into(), small: "haiku".into() }, api: ApiConfig { provider: provider.into(), base_url: String::new(), main: main.into(), small: "s".into(), daily_cap_usd: 2.0 }, key: None }
    }

    #[test]
    fn an_api_save_needs_models_and_a_key() {
        let c = Config::default();
        assert!(apply(&c, &save("api", "openrouter", ""), true).unwrap_err().contains("main model"));
        assert_eq!(apply(&c, &save("api", "openrouter", "m"), false).unwrap_err(), "Add your OpenRouter key.");
        let out = apply(&c, &save("api", "openrouter", "m"), true).unwrap();
        assert_eq!((out.runtime.as_str(), out.api.base_url.as_str(), out.api.daily_cap_usd), ("api", "https://openrouter.ai/api/v1", 2.0));
        assert_eq!(out.tiers, c.tiers, "Claude Code tiers stay as they were");
        assert!(apply(&c, &save("api", "ollama", "llama3.1"), false).is_ok(), "a local server needs no key");
    }

    #[test]
    fn an_api_save_checks_the_base_url() {
        let c = Config::default();
        let mut s = save("api", "custom", "m");
        assert!(apply(&c, &s, true).unwrap_err().contains("base URL"));
        s.api.base_url = "http://example.com/v1".into();
        assert!(apply(&c, &s, true).unwrap_err().starts_with("Use an https://"));
        s.api.base_url = "https://llm.example.com/v1/".into();
        assert_eq!(apply(&c, &s, true).unwrap().api.base_url, "https://llm.example.com/v1");
        let mut s = save("api", "openrouter", "m");
        s.api.daily_cap_usd = -1.0;
        assert!(apply(&c, &s, true).unwrap_err().contains("daily cap"));
    }

    #[test]
    fn a_claude_save_takes_only_claude_models() {
        let c = Config::default();
        let out = apply(&c, &save("claude", "openrouter", ""), false).unwrap();
        assert_eq!((out.runtime.as_str(), out.tiers.main.as_str()), ("claude", "opus"));
        let mut s = save("claude", "", "");
        s.tiers.main = "gpt-5".into();
        assert!(apply(&c, &s, false).is_err());
        assert!(apply(&c, &save("codex", "", ""), false).is_err());
    }

    #[test]
    fn saved_keys_lists_providers_never_values() {
        let s = MemorySecrets::default();
        s.set("api:anthropic:key", "sk-secret").unwrap();
        s.set("api:openai:key", "").unwrap();
        assert_eq!(saved_keys(&s), ["anthropic"]);
    }
}
