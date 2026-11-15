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

/// Providers that take a key. `custom` takes one if the server needs it.
pub fn takes_key(p: &Preset) -> bool {
    p.needs_key || p.id == "custom"
}

/// Where calls go. A provider with a key of its own always uses its own address, so a saved
/// key never follows a typed URL. Others use the typed URL, else the preset's.
pub fn base_for<'a>(p: &'a Preset, typed: &'a str) -> &'a str {
    let t = typed.trim().trim_end_matches('/');
    if p.needs_key || t.is_empty() { p.base_url } else { t }
}

/// A base URL bleep may send a key to: https, or http only to this Mac. No user name,
/// password or query, so no secret hides in the URL.
pub fn check_base_url(url: &str) -> Result<(), String> {
    let bad = || "Use an https:// address, or http:// only for a server on this Mac.".to_string();
    let u = reqwest::Url::parse(url.trim()).map_err(|_| bad())?;
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || u.fragment().is_some() {
        return Err("Remove the user name, password or query from the address.".into());
    }
    let host = u.host_str().unwrap_or("");
    match u.scheme() {
        "https" if !host.is_empty() => Ok(()),
        "http" if ["localhost", "127.0.0.1", "[::1]"].contains(&host) => Ok(()),
        _ => Err(bad()),
    }
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
    fn a_keyed_provider_always_uses_its_own_address() {
        let openai = preset("openai").unwrap();
        assert_eq!(base_for(openai, "https://x.io/v1"), "https://api.openai.com/v1");
        assert_eq!(base_for(preset("custom").unwrap(), " https://llm.example.com/v1/ "), "https://llm.example.com/v1");
        assert_eq!(base_for(preset("ollama").unwrap(), ""), "http://localhost:11434/v1");
        assert!(takes_key(preset("custom").unwrap()) && !takes_key(preset("ollama").unwrap()));
    }

    #[test]
    fn keys_go_only_to_https_or_this_mac() {
        assert!(check_base_url("http://localhost:1@evil.com/v1").is_err(), "userinfo hides the real host");
        assert!(check_base_url("https://user:pw@x.io/v1").is_err());
        assert!(check_base_url("https://x.io/v1?key=abc").is_err(), "no secrets in the URL");
        assert!(check_base_url("https://").is_err());
        assert!(check_base_url("https://x.io/v1").is_ok());
        assert!(check_base_url("http://localhost:11434/v1").is_ok());
        assert!(check_base_url("http://127.0.0.1:1234/v1").is_ok());
        assert!(check_base_url("http://localhost.evil.com/v1").is_err());
        assert!(check_base_url("http://example.com/v1").is_err());
        assert!(check_base_url("ftp://x").is_err());
    }
}
