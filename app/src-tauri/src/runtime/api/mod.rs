//! bleep's own agent loop over a provider API, for people who bring their own key.
pub mod adapter;
pub mod anthropic;
pub mod openai;
pub mod presets;
pub mod prices;
pub mod types;
pub mod agent;
#[cfg(test)]
mod http_tests;
