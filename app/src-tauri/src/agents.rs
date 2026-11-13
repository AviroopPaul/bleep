//! Tauri commands for the Agents view (chat panel) and the Agents settings tab.
use crate::agentwatch::codex::{daemon_running, CodexWatch};
use crate::agentwatch::install::{self, HookFiles};
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
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentSettings {
    pub claude_hooks: bool,
    pub codex_notify: bool,
    pub codex_running: bool,
    pub auto_approve: bool,
    pub screenshot_check: bool,
    pub codex_watch: bool,
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
pub fn agents_list(state: tauri::State<'_, AppState>) -> Result<AgentsView, String> {
    let sessions = state.store.agent_sessions(chrono::Utc::now()).map_err(|e| e.to_string())?;
    let asks = state.store.agent_asks_recent(50).map_err(|e| e.to_string())?;
    Ok(AgentsView { sessions, asks })
}

/// My tap on a queued ask. Only Codex asks can be answered from bleep.
#[tauri::command]
pub async fn agents_decide(codex: tauri::State<'_, Arc<CodexWatch>>, ask_id: i64, approve: bool) -> Result<(), String> {
    codex.answer(ask_id, approve).await
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
pub fn agents_set(state: tauri::State<'_, AppState>, auto_approve: bool, screenshot_check: bool, codex_watch: bool) -> Result<AgentSettings, String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.agents = AgentsConfig { auto_approve, screenshot_check, codex_watch };
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
        let cfg = AgentsConfig { auto_approve: true, screenshot_check: false, codex_watch: true };
        let rules = d.path().join("agent-rules.toml");
        let s = snapshot(&files, &cfg, d.path(), &rules);
        assert_eq!(s, AgentSettings { claude_hooks: false, codex_notify: false, codex_running: false, auto_approve: true, screenshot_check: false, codex_watch: true, rules_path: rules.display().to_string() });
        install::install_claude(&files).unwrap();
        install::install_codex_notify(&files).unwrap();
        let s = snapshot(&files, &cfg, d.path(), &rules);
        assert!(s.claude_hooks && s.codex_notify);
    }
}
