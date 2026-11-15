//! Providers bleep knows. A preset is data: which wire format, where, and whether a key is
//! needed. Any other OpenAI-compatible server uses `custom` with its own base URL.
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Wire {
    Anthropic,
    OpenAi,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub wire: Wire,
    pub base_url: &'static str,
    pub needs_key: bool,
    /// Where to get a key, shown under the key field.
    pub key_url: &'static str,
    pub main: &'static str,
    pub small: &'static str,
    /// OpenAI's newer models take `max_completion_tokens`; most other servers take `max_tokens`.
    pub max_completion_tokens: bool,
    /// Calls cost nothing (a model on this Mac).
    pub free: bool,
}

pub const PRESETS: &[Preset] = &[
    Preset { id: "anthropic", label: "Anthropic", wire: Wire::Anthropic, base_url: "https://api.anthropic.com", needs_key: true, key_url: "https://console.anthropic.com/settings/keys", main: "claude-sonnet-5-5", small: "claude-haiku-5-5", max_completion_tokens: false, free: false },
    Preset { id: "openai", label: "OpenAI", wire: Wire::OpenAi, base_url: "https://api.openai.com/v1", needs_key: true, key_url: "https://platform.openai.com/api-keys", main: "gpt-5", small: "gpt-5-mini", max_completion_tokens: true, free: false },
    Preset { id: "gemini", label: "Google Gemini", wire: Wire::OpenAi, base_url: "https://generativelanguage.googleapis.com/v1beta/openai", needs_key: true, key_url: "https://aistudio.google.com/apikey", main: "gemini-2.5-pro", small: "gemini-2.5-flash", max_completion_tokens: false, free: false },
    Preset { id: "openrouter", label: "OpenRouter", wire: Wire::OpenAi, base_url: "https://openrouter.ai/api/v1", needs_key: true, key_url: "https://openrouter.ai/settings/keys", main: "", small: "", max_completion_tokens: false, free: false },
    Preset { id: "groq", label: "Groq", wire: Wire::OpenAi, base_url: "https://api.groq.com/openai/v1", needs_key: true, key_url: "https://console.groq.com/keys", main: "", small: "", max_completion_tokens: false, free: false },
    Preset { id: "ollama", label: "Ollama (on this Mac)", wire: Wire::OpenAi, base_url: "http://localhost:11434/v1", needs_key: false, key_url: "", main: "", small: "", max_completion_tokens: false, free: true },
    Preset { id: "lmstudio", label: "LM Studio (on this Mac)", wire: Wire::OpenAi, base_url: "http://localhost:1234/v1", needs_key: false, key_url: "", main: "", small: "", max_completion_tokens: false, free: true },
    Preset { id: "custom", label: "Other (OpenAI-compatible)", wire: Wire::OpenAi, base_url: "", needs_key: false, key_url: "", main: "", small: "", max_completion_tokens: false, free: false },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// A base URL bleep may send a key to: https, or http only to this Mac.
pub fn check_base_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    if u.starts_with("https://") {
        return Ok(());
    }
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"];
    if local.iter().any(|p| u.starts_with(p) && matches!(u[p.len()..].chars().next(), None | Some(':') | Some('/'))) {
        return Ok(());
    }
    Err("Use an https:// address, or http:// only for a server on this Mac.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_well_formed() {
        let mut ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len(), "unique ids");
        for p in PRESETS {
            assert!(p.base_url.is_empty() || check_base_url(p.base_url).is_ok(), "{}", p.id);
            assert!(!p.base_url.ends_with('/'), "{} has no trailing slash", p.id);
            assert!(!p.label.contains('\u{2014}'));
            assert_eq!(p.needs_key, !p.key_url.is_empty(), "{}", p.id);
        }
        assert_eq!(preset("anthropic").unwrap().wire, Wire::Anthropic);
        assert!(preset("nope").is_none());
    }

    #[test]
    fn keys_go_only_to_https_or_this_mac() {
        assert!(check_base_url("https://x.io/v1").is_ok());
        assert!(check_base_url("http://localhost:11434/v1").is_ok());
        assert!(check_base_url("http://127.0.0.1:1234/v1").is_ok());
        assert!(check_base_url("http://localhost.evil.com/v1").is_err());
        assert!(check_base_url("http://example.com/v1").is_err());
        assert!(check_base_url("ftp://x").is_err());
    }
}
