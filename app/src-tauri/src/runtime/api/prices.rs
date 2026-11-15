//! Built-in prices (US dollars per million tokens) for common models whose provider does not
//! publish prices in its model list. OpenRouter's list carries its own. A model not here has
//! no estimate. Checked against the providers' price pages in November 2026.
use super::types::Price;

/// More specific prefixes come first.
const TABLE: &[(&str, f64, f64, f64, f64)] = &[
    // id prefix, input, output, cache read, cache write
    ("claude-opus-4-5", 5.0, 25.0, 0.5, 6.25),
    ("claude-opus-4", 15.0, 75.0, 1.5, 18.75),
    ("claude-sonnet-4", 3.0, 15.0, 0.3, 3.75),
    ("claude-3-7-sonnet", 3.0, 15.0, 0.3, 3.75),
    ("claude-haiku-4-5", 1.0, 5.0, 0.1, 1.25),
    ("claude-3-5-haiku", 0.8, 4.0, 0.08, 1.0),
    ("gpt-5-nano", 0.05, 0.4, 0.005, 0.0),
    ("gpt-5-mini", 0.25, 2.0, 0.025, 0.0),
    ("gpt-5", 1.25, 10.0, 0.125, 0.0),
    ("gpt-4-1-nano", 0.1, 0.4, 0.025, 0.0),
    ("gpt-4-1-mini", 0.4, 1.6, 0.1, 0.0),
    ("gpt-4-1", 2.0, 8.0, 0.5, 0.0),
    ("gpt-4o-mini", 0.15, 0.6, 0.075, 0.0),
    ("gpt-4o", 2.5, 10.0, 1.25, 0.0),
    ("gemini-2-5-flash-lite", 0.1, 0.4, 0.025, 0.0),
    ("gemini-2-5-flash", 0.3, 2.5, 0.075, 0.0),
    ("gemini-2-5-pro", 1.25, 10.0, 0.31, 0.0),
];

/// `anthropic/claude-sonnet-4.5` and `claude-sonnet-4-5-20250929` both match `claude-sonnet-4`.
pub fn builtin(model: &str) -> Option<Price> {
    let id = model.rsplit('/').next().unwrap_or(model).replace('.', "-").to_lowercase();
    TABLE.iter().find(|(p, ..)| id.starts_with(p)).map(|&(_, input, output, cache_read, cache_write)| Price { input, output, cache_read, cache_write })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_match_dated_and_routed_ids() {
        assert_eq!(builtin("claude-sonnet-4-5-20250929").unwrap().input, 3.0);
        assert_eq!(builtin("anthropic/claude-opus-4.5").unwrap().input, 5.0, "4.5 is not the 4 price");
        assert_eq!(builtin("gpt-5-mini-2025-08-07").unwrap().output, 2.0, "mini before the plain prefix");
        assert_eq!(builtin("models/gemini-2.5-flash").unwrap().output, 2.5);
        assert!(builtin("llama3.1:8b").is_none());
    }
}
