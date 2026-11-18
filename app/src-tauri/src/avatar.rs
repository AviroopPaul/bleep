//! The floating avatar: which buddy, its color and its finish. The views draw it
//! (`src/avatar/avatars.ts`); this module saves the choice, tells every window,
//! and swaps the menu bar icon to the buddy's outline.

use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

/// Buddies and their default colors. "droid" is the original bleep droid.
pub const KINDS: [(&str, &str); 7] = [
    ("blip", "#3D6BFF"),
    ("gumdrop", "#1FC48E"),
    ("sprout", "#FF6B4A"),
    ("boop", "#8B5CF6"),
    ("puff", "#38BDF8"),
    ("spark", "#FFB020"),
    ("droid", "#2F5C93"),
];
pub const FINISHES: [&str; 3] = ["flat", "glossy", "plush"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AvatarConfig {
    pub kind: String,
    /// "#RRGGBB".
    pub color: String,
    /// "flat", "glossy" or "plush".
    pub finish: String,
}

impl Default for AvatarConfig {
    fn default() -> Self {
        Self { kind: "blip".into(), color: "#3D6BFF".into(), finish: "glossy".into() }
    }
}

fn is_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl AvatarConfig {
    /// Unknown values fall back to the defaults, so a hand-edited config never breaks the views.
    pub fn normalized(self) -> Self {
        let Some((kind, default_color)) = KINDS.iter().find(|(k, _)| *k == self.kind) else {
            return Self::default();
        };
        let color = if is_hex(&self.color) { self.color.to_ascii_uppercase() } else { (*default_color).into() };
        let finish = if FINISHES.contains(&self.finish.as_str()) { self.finish } else { "glossy".into() };
        Self { kind: (*kind).into(), color, finish }
    }
}

/// The buddy's outline for the menu bar. macOS uses it as a template image, so only the
/// alpha counts. Sources: `icons/tray/*.svg`, drawn by `scripts/tray-icons.mjs`.
pub fn tray_icon(kind: &str) -> Image<'static> {
    match kind {
        "blip" => tauri::include_image!("icons/tray/blip.png"),
        "gumdrop" => tauri::include_image!("icons/tray/gumdrop.png"),
        "sprout" => tauri::include_image!("icons/tray/sprout.png"),
        "boop" => tauri::include_image!("icons/tray/boop.png"),
        "puff" => tauri::include_image!("icons/tray/puff.png"),
        "spark" => tauri::include_image!("icons/tray/spark.png"),
        _ => tauri::include_image!("icons/tray.png"),
    }
}

pub fn current(app: &AppHandle) -> AvatarConfig {
    app.state::<AppState>().config.lock().unwrap().avatar.clone().normalized()
}

#[tauri::command]
pub fn avatar_get(app: AppHandle) -> AvatarConfig {
    current(&app)
}

/// Saves the avatar, redraws it in every window and swaps the menu bar icon.
#[tauri::command]
pub fn avatar_set(app: AppHandle, config: AvatarConfig) -> Result<AvatarConfig, String> {
    let c = config.normalized();
    {
        let st = app.state::<AppState>();
        let mut cfg = st.config.lock().unwrap();
        cfg.avatar = c.clone();
        cfg.save(&st.paths.config_file).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("avatar://changed", &c);
    if let Some(tray) = app.tray_by_id("bleep") {
        let _ = tray.set_icon(Some(tray_icon(&c.kind)));
        let _ = tray.set_icon_as_template(true);
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_blip() {
        let c: crate::config::Config = toml::from_str("").unwrap();
        assert_eq!(c.avatar, AvatarConfig::default());
        let c: crate::config::Config = toml::from_str("[avatar]\nkind = \"puff\"\nfinish = \"plush\"").unwrap();
        assert_eq!((c.avatar.kind.as_str(), c.avatar.finish.as_str()), ("puff", "plush"));
    }

    #[test]
    fn normalizes_bad_values() {
        let c = AvatarConfig { kind: "sprout".into(), color: "red".into(), finish: "shiny".into() }.normalized();
        assert_eq!(c, AvatarConfig { kind: "sprout".into(), color: "#FF6B4A".into(), finish: "glossy".into() });
        let c = AvatarConfig { kind: "dragon".into(), color: "#123456".into(), finish: "flat".into() }.normalized();
        assert_eq!(c, AvatarConfig::default());
        let c = AvatarConfig { kind: "boop".into(), color: "#ff4fa3".into(), finish: "flat".into() }.normalized();
        assert_eq!(c.color, "#FF4FA3");
    }

    #[test]
    fn every_kind_has_a_tray_icon() {
        for (k, _) in KINDS {
            let img = tray_icon(k);
            assert!(img.width() > 0 && img.height() > 0, "{k}");
        }
    }
}
