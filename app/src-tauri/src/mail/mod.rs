//! Mail watch and sorting (build plan 8.5): watch Gmail, label with rules first, a small
//! model only when unsure, and find threads waiting on the user.
pub mod pipeline;
pub mod rules;
pub mod triage;
pub mod waiting;
pub mod watch;
