//! Tauri commands for Settings > Memory and the chat's context meter.
use crate::config::{MemoryConfig, MIN_COMPACT_AT};
use crate::memory::{Memory, PROMPT_BUDGET};
use crate::paths::MemFile;
use crate::store::{Proposal, Store};
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;

pub const MAX_COMPACT_AT: u64 = 1_000_000;

#[derive(Serialize, Debug)]
pub struct FileView {
    pub file: &'static str,
    pub facts: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct MemoryView {
    pub files: Vec<FileView>,
    pub size: usize,
    pub budget: usize,
}

#[derive(Serialize, Debug)]
pub struct MemorySettingsView {
    pub compact_at: u64,
    pub threshold: u64,
    pub window: Option<u64>,
    pub nightly: String,
}

#[derive(Serialize, Debug)]
pub struct ChatContext {
    pub used: Option<u64>,
    pub threshold: u64,
    pub window: Option<u64>,
}

pub fn view(mem: &Memory) -> MemoryView {
    let files = mem.facts().into_iter().map(|f| FileView { file: f.file.file_name(), facts: f.facts }).collect();
    MemoryView { files, size: mem.size(), budget: PROMPT_BUDGET }
}

pub fn forget(mem: &Memory, file: &str, fact: &str) -> Result<MemoryView, String> {
    let f = MemFile::from_name(file).ok_or_else(|| format!("Unknown memory file: {file}"))?;
    mem.forget_fact(f, fact).map_err(|e| format!("Could not change {file}: {e}"))?;
    Ok(view(mem))
}

/// Add writes MEMORY.md first and only then marks the suggestion added; a full memory keeps it.
pub fn decide(store: &Store, mem: &Memory, id: i64, add: bool) -> Result<Vec<Proposal>, String> {
    let e = |e: anyhow::Error| e.to_string();
    if add {
        if let Some(p) = store.pending_proposals().map_err(e)?.into_iter().find(|p| p.id == id) {
            // Already known counts as added: the fact is in memory either way.
            mem.add_fact(MemFile::Memory, &p.fact)?;
        }
    }
    store.decide_proposal(id, add).map_err(e)?;
    store.pending_proposals().map_err(e)
}

pub fn settings_view(c: &MemoryConfig) -> MemorySettingsView {
    MemorySettingsView { compact_at: c.compact_at, threshold: c.threshold(), window: c.context_window, nightly: c.nightly.clone() }
}

pub fn clamp_compact_at(v: u64) -> u64 {
    v.clamp(MIN_COMPACT_AT, MAX_COMPACT_AT)
}

#[tauri::command]
pub fn memory_list(state: tauri::State<'_, AppState>) -> MemoryView {
    view(&state.memory)
}

#[tauri::command]
pub fn memory_forget(state: tauri::State<'_, AppState>, file: String, fact: String) -> Result<MemoryView, String> {
    forget(&state.memory, &file, &fact)
}

#[tauri::command]
pub fn memory_suggestions(state: tauri::State<'_, AppState>) -> Result<Vec<Proposal>, String> {
    state.store.pending_proposals().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn memory_decide(state: tauri::State<'_, AppState>, id: i64, add: bool) -> Result<Vec<Proposal>, String> {
    decide(&state.store, &state.memory, id, add)
}

#[tauri::command]
pub fn memory_settings(state: tauri::State<'_, AppState>) -> MemorySettingsView {
    settings_view(&state.config.lock().unwrap().memory)
}

#[tauri::command]
pub fn memory_set(state: tauri::State<'_, AppState>, compact_at: u64) -> Result<MemorySettingsView, String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.memory.compact_at = clamp_compact_at(compact_at);
    cfg.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    Ok(settings_view(&cfg.memory))
}

#[tauri::command]
pub async fn memory_compact_now(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let chat: Arc<crate::chat::ChatService> = state.chat.clone();
    chat.compact_now().await.map_err(|e| e.to_string())
}

/// Shows MEMORY.md in Finder, in the memory folder.
#[tauri::command]
pub fn memory_reveal(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = state.memory.ensure();
    tauri_plugin_opener::reveal_item_in_dir(state.paths.memory_file(MemFile::Memory)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_context(state: tauri::State<'_, AppState>) -> Result<ChatContext, String> {
    let (threshold, window) = {
        let c = state.config.lock().unwrap();
        (c.memory.threshold(), c.memory.context_window)
    };
    let chat = state.chat.clone();
    Ok(ChatContext { used: chat.context().await, threshold, window })
}

#[cfg(test)]
mod tests;
