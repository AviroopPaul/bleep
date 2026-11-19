//! Tauri commands for the Agents view (chat panel) and the Agents settings tab.
use crate::agentwatch::codex::{daemon_running, CodexWatch};
use crate::agentwatch::hub::AgentWatch;
use crate::agentwatch::transcript::{read_chat, ChatLine, CHAT_BYTES, CHAT_LINES};
use crate::agentwatch::install::{self, HookFiles};
use crate::agentwatch::answer::AnswerMode;
use crate::config::AgentsConfig;
use crate::store::{AgentAsk, AgentSession};
use crate::AppState;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

#[derive(Serialize)]
pub struct AgentsView {
    pub sessions: Vec<AgentSession>,
    pub asks: Vec<AgentAsk>,
    /// Claude Code asks whose hook waits for my answer in bleep.
    pub held: Vec<i64>,
    /// Claude Code sessions that take a reply from bleep.
    pub replyable: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentSettings {
    pub claude_hooks: bool,
    pub codex_notify: bool,
    pub codex_running: bool,
    pub auto_approve: bool,
    pub screenshot_check: bool,
    pub codex_watch: bool,
    pub auto_answer: AnswerMode,
    pub rules_path: String,
}

pub fn snapshot(files: &HookFiles, cfg: &AgentsConfig, codex_home: &Path, rules: &Path) -> AgentSettings {
    AgentSettings {
        claude_hooks: install::claude_status(files),
        codex_notify: install::codex_notify_status(files),
        codex_running: daemon_running(codex_home),
        auto_approve: cfg.auto_approve,
        screenshot_check: cfg.screenshot_check,
        codex_watch: cfg.codex_watch,
        auto_answer: cfg.auto_answer,
        rules_path: rules.display().to_string(),
    }
}

pub fn codex_home() -> std::path::PathBuf {
    std::env::var("CODEX_HOME").ok().filter(|s| !s.is_empty()).map(Into::into).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
}

fn now_settings(state: &AppState) -> AgentSettings {
    let cfg = state.config.lock().unwrap().agents.clone();
    snapshot(&HookFiles::real(&state.paths), &cfg, &codex_home(), &state.paths.agent_rules)
}

#[tauri::command]
pub fn agents_list(state: tauri::State<'_, AppState>, watch: tauri::State<'_, Arc<AgentWatch>>) -> Result<AgentsView, String> {
    let sessions = state.store.agent_sessions(chrono::Utc::now()).map_err(|e| e.to_string())?;
    let asks = state.store.agent_asks_recent(50).map_err(|e| e.to_string())?;
    let live = watch.live();
    Ok(AgentsView { sessions, asks, held: live.held, replyable: live.replyable })
}

/// My tap on a queued ask: a held Claude Code permission, or a Codex ask.
#[tauri::command]
pub async fn agents_decide(codex: tauri::State<'_, Arc<CodexWatch>>, watch: tauri::State<'_, Arc<AgentWatch>>, ask_id: i64, approve: bool) -> Result<(), String> {
    if watch.live().held.contains(&ask_id) {
        return watch.decide_held(ask_id, approve);
    }
    codex.answer(ask_id, approve).await
}

/// My picks for a Claude Code AskUserQuestion: question text to option label.
#[tauri::command]
pub fn agents_choose(watch: tauri::State<'_, Arc<AgentWatch>>, ask_id: i64, answers: serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    watch.choose(ask_id, answers)
}

/// My reply to a Claude Code session that stopped. It wakes the session with my words.
#[tauri::command]
pub fn agents_reply(watch: tauri::State<'_, Arc<AgentWatch>>, session_id: String, text: String) -> Result<(), String> {
    watch.reply(&session_id, &text)
}

/// My stop on a pending auto-answer: the ask stays for me.
#[tauri::command]
pub fn agents_keep(watch: tauri::State<'_, Arc<AgentWatch>>, ask_id: i64) -> Result<(), String> {
    watch.keep(ask_id)
}

/// The newest lines of a session's chat, read from its Claude Code transcript.
#[tauri::command]
pub async fn agents_chat(state: tauri::State<'_, AppState>, session_id: String) -> Result<Vec<ChatLine>, String> {
    let s = state.store.agent_session(&session_id).map_err(|e| e.to_string())?.ok_or("No such session.")?;
    let Some(path) = s.transcript_path.filter(|p| p.ends_with(".jsonl")) else { return Ok(vec![]) };
    tauri::async_runtime::spawn_blocking(move || read_chat(Path::new(&path), CHAT_BYTES, CHAT_LINES)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agents_settings(state: tauri::State<'_, AppState>) -> AgentSettings {
    now_settings(&state)
}

#[tauri::command]
pub fn agents_install_claude(state: tauri::State<'_, AppState>) -> Result<AgentSettings, String> {
    install::install_claude(&HookFiles::real(&state.paths))?;
    Ok(now_settings(&state))
}

#[tauri::command]
pub fn agents_remove_claude(state: tauri::State<'_, AppState>) -> Result<AgentSettings, String> {
    install::remove_claude(&HookFiles::real(&state.paths))?;
    Ok(now_settings(&state))
}

#[tauri::command]
pub fn agents_install_codex_notify(state: tauri::State<'_, AppState>) -> Result<AgentSettings, String> {
    install::install_codex_notify(&HookFiles::real(&state.paths))?;
    Ok(now_settings(&state))
}

#[tauri::command]
pub fn agents_remove_codex_notify(state: tauri::State<'_, AppState>) -> Result<AgentSettings, String> {
    install::remove_codex_notify(&HookFiles::real(&state.paths))?;
    Ok(now_settings(&state))
}

#[tauri::command]
pub fn agents_set(state: tauri::State<'_, AppState>, auto_approve: bool, screenshot_check: bool, codex_watch: bool, auto_answer: Option<AnswerMode>) -> Result<AgentSettings, String> {
    {
        let mut cfg = state.config.lock().unwrap();
        let auto_answer = auto_answer.unwrap_or(cfg.agents.auto_answer);
        cfg.agents = AgentsConfig { auto_approve, screenshot_check, codex_watch, auto_answer, ..cfg.agents.clone() };
        cfg.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    }
    Ok(now_settings(&state))
}

/// Shows `agent-rules.toml` in Finder (written with the defaults if missing).
#[tauri::command]
pub fn agents_reveal_rules(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = crate::agentwatch::rules::load_rules(&state.paths.agent_rules);
    tauri_plugin_opener::reveal_item_in_dir(&state.paths.agent_rules).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn snapshot_reads_files_and_config() {
        let d = tempfile::tempdir().unwrap();
        let files = HookFiles { claude_settings: d.path().join("c/settings.json"), codex_config: d.path().join("x/config.toml"), hook_bin: PathBuf::from("/b/bleep-hook") };
        let cfg = AgentsConfig { auto_approve: true, screenshot_check: false, codex_watch: true, auto_answer: AnswerMode::On, answer_delay_secs: 0 };
        let rules = d.path().join("agent-rules.toml");
        let s = snapshot(&files, &cfg, d.path(), &rules);
        assert_eq!(s, AgentSettings { claude_hooks: false, codex_notify: false, codex_running: false, auto_approve: true, screenshot_check: false, codex_watch: true, auto_answer: AnswerMode::On, rules_path: rules.display().to_string() });
        install::install_claude(&files).unwrap();
        install::install_codex_notify(&files).unwrap();
        let s = snapshot(&files, &cfg, d.path(), &rules);
        assert!(s.claude_hooks && s.codex_notify);
    }
}
