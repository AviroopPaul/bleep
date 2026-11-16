//! Self-update. bleep checks its public release feed (bleep-releases on GitHub) with the Tauri
//! updater, which checks the minisign signature, swaps the app bundle in place, and leaves the
//! new version to start at the next restart. The flow below is plain logic over a `Source`, so
//! tests run it without the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;

/// First check after launch, so start-up work is not slowed.
pub const FIRST_CHECK: Duration = Duration::from_secs(120);
/// Then a check every few hours while bleep runs.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, notes: Option<String> },
    Downloading { version: String, done: u64, total: Option<u64> },
    /// Installed on disk. The new version runs after a restart.
    Ready { version: String },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UpdateView {
    pub current: String,
    pub auto_install: bool,
    pub last_checked: Option<DateTime<Utc>>,
    pub phase: Phase,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub version: String,
    pub notes: Option<String>,
}

pub type Progress = Box<dyn FnMut(u64, Option<u64>) + Send>;

/// Where updates come from. `install` installs the update the last `check` found.
#[async_trait]
pub trait Source: Send + Sync {
    async fn check(&self) -> Result<Option<Found>, String>;
    async fn install(&self, progress: Progress) -> Result<(), String>;
}

pub struct Deps {
    pub source: Arc<dyn Source>,
    pub current: String,
    pub auto_install: Arc<dyn Fn() -> bool + Send + Sync>,
    pub emit: Arc<dyn Fn(UpdateView) + Send + Sync>,
    pub now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

pub struct Updater {
    deps: Deps,
    state: Mutex<(Phase, Option<DateTime<Utc>>)>,
    busy: AtomicBool,
}

impl Updater {
    pub fn new(deps: Deps) -> Arc<Updater> {
        Arc::new(Updater { deps, state: Mutex::new((Phase::Idle, None)), busy: AtomicBool::new(false) })
    }

    pub fn view(&self) -> UpdateView {
        let (phase, last_checked) = self.state.lock().unwrap().clone();
        UpdateView { current: self.deps.current.clone(), auto_install: (self.deps.auto_install)(), last_checked, phase }
    }

    fn set(&self, phase: Phase) {
        self.state.lock().unwrap().0 = phase;
        (self.deps.emit)(self.view());
    }

    /// Looks for a new version. With auto-install on, it also downloads and installs it.
    /// Does nothing while another check or download runs, or once an update waits for a restart.
    pub async fn check(self: &Arc<Self>) -> UpdateView {
        if matches!(self.state.lock().unwrap().0, Phase::Ready { .. }) || self.busy.swap(true, Ordering::SeqCst) {
            return self.view();
        }
        self.set(Phase::Checking);
        let res = self.deps.source.check().await;
        self.state.lock().unwrap().1 = Some((self.deps.now)());
        let found = match res {
            Ok(None) => {
                self.set(Phase::UpToDate);
                None
            }
            Ok(Some(f)) => {
                self.set(Phase::Available { version: f.version.clone(), notes: f.notes.clone() });
                Some(f)
            }
            Err(e) => {
                self.set(Phase::Error { message: check_error(&e) });
                None
            }
        };
        let install = found.is_some() && (self.deps.auto_install)();
        self.busy.store(false, Ordering::SeqCst);
        if install {
            return self.install().await;
        }
        self.view()
    }

    /// Downloads and installs the update that `check` found.
    pub async fn install(self: &Arc<Self>) -> UpdateView {
        let Phase::Available { version, .. } = self.state.lock().unwrap().0.clone() else {
            return self.view();
        };
        if self.busy.swap(true, Ordering::SeqCst) {
            return self.view();
        }
        self.set(Phase::Downloading { version: version.clone(), done: 0, total: None });
        let emit: Progress = {
            let me = self.clone();
            let version = version.clone();
            let (mut done, mut last_step) = (0u64, 0u64);
            Box::new(move |chunk, total| {
                done += chunk;
                let step = step_of(done, total);
                if step != last_step {
                    last_step = step;
                    me.set(Phase::Downloading { version: version.clone(), done, total });
                }
            })
        };
        let res = self.deps.source.install(emit).await;
        match res {
            Ok(()) => self.set(Phase::Ready { version }),
            Err(e) => self.set(Phase::Error { message: format!("Could not install {version}: {e}") }),
        }
        self.busy.store(false, Ordering::SeqCst);
        self.view()
    }
}

/// Progress events go out once per percent (or per MB when the size is unknown), not per chunk.
fn step_of(done: u64, total: Option<u64>) -> u64 {
    match total {
        Some(t) if t > 0 => done * 100 / t,
        _ => done / (1024 * 1024),
    }
}

fn check_error(e: &str) -> String {
    format!("Could not check for updates: {e}")
}

/// The real source: the Tauri updater plugin, configured in `tauri.conf.json` (`plugins.updater`).
pub struct TauriSource {
    app: tauri::AppHandle,
    pending: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
}

impl TauriSource {
    pub fn new(app: tauri::AppHandle) -> Arc<TauriSource> {
        Arc::new(TauriSource { app, pending: tokio::sync::Mutex::new(None) })
    }
}

#[async_trait]
impl Source for TauriSource {
    async fn check(&self) -> Result<Option<Found>, String> {
        use tauri_plugin_updater::UpdaterExt;
        let up = self.app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
        let found = up.as_ref().map(|u| Found { version: u.version.clone(), notes: u.body.clone().filter(|b| !b.trim().is_empty()) });
        *self.pending.lock().await = up;
        Ok(found)
    }

    async fn install(&self, mut progress: Progress) -> Result<(), String> {
        let pending = self.pending.lock().await;
        let up = pending.as_ref().ok_or("no update to install")?;
        up.download_and_install(|chunk, total| progress(chunk as u64, total), || {}).await.map_err(|e| e.to_string())
    }
}

/// Builds the updater, keeps it in app state, and checks on a timer in release builds.
pub fn start(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let (h1, h2) = (app.clone(), app.clone());
    let up = Updater::new(Deps {
        source: TauriSource::new(app.clone()),
        current: app.package_info().version.to_string(),
        auto_install: Arc::new(move || h1.state::<crate::AppState>().config.lock().unwrap().updates.auto_install),
        emit: Arc::new(move |v: UpdateView| {
            let _ = h2.emit("update://changed", v);
        }),
        now: Arc::new(Utc::now),
    });
    app.manage(up.clone());
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            up.check().await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

type Up<'a> = tauri::State<'a, Arc<Updater>>;

#[tauri::command]
pub fn update_state(up: Up<'_>) -> UpdateView {
    up.view()
}

#[tauri::command]
pub async fn update_check(up: Up<'_>) -> Result<UpdateView, String> {
    Ok(up.inner().clone().check().await)
}

#[tauri::command]
pub async fn update_install(up: Up<'_>) -> Result<UpdateView, String> {
    Ok(up.inner().clone().install().await)
}

#[tauri::command]
pub fn update_set_auto(up: Up<'_>, state: tauri::State<'_, crate::AppState>, on: bool) -> Result<UpdateView, String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.updates.auto_install = on;
        cfg.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    }
    Ok(up.view())
}

#[tauri::command]
pub fn update_restart(app: tauri::AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Fake {
        checks: Mutex<VecDeque<Result<Option<Found>, String>>>,
        install: Mutex<Option<Result<(), String>>>,
        installs: Mutex<u32>,
    }

    impl Fake {
        fn new(checks: Vec<Result<Option<Found>, String>>, install: Result<(), String>) -> Arc<Fake> {
            Arc::new(Fake { checks: Mutex::new(checks.into()), install: Mutex::new(Some(install)), installs: Mutex::new(0) })
        }
    }

    #[async_trait]
    impl Source for Fake {
        async fn check(&self) -> Result<Option<Found>, String> {
            self.checks.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }
        async fn install(&self, mut progress: Progress) -> Result<(), String> {
            *self.installs.lock().unwrap() += 1;
            progress(50, Some(100));
            progress(50, Some(100));
            self.install.lock().unwrap().take().unwrap_or(Ok(()))
        }
    }

    fn found(v: &str) -> Found {
        Found { version: v.into(), notes: Some("New things".into()) }
    }

    fn updater(src: Arc<Fake>, auto: bool) -> (Arc<Updater>, Arc<Mutex<Vec<Phase>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let u = Updater::new(Deps {
            source: src,
            current: "0.2.0".into(),
            auto_install: Arc::new(move || auto),
            emit: Arc::new(move |v: UpdateView| s.lock().unwrap().push(v.phase)),
            now: Arc::new(|| DateTime::parse_from_rfc3339("2026-11-16T10:00:00Z").unwrap().with_timezone(&Utc)),
        });
        (u, seen)
    }

    #[tokio::test]
    async fn up_to_date() {
        let (u, seen) = updater(Fake::new(vec![Ok(None)], Ok(())), true);
        let v = u.check().await;
        assert_eq!(v.phase, Phase::UpToDate);
        assert_eq!(v.current, "0.2.0");
        assert!(v.last_checked.is_some());
        assert_eq!(*seen.lock().unwrap(), vec![Phase::Checking, Phase::UpToDate]);
    }

    #[tokio::test]
    async fn auto_install_downloads_and_waits_for_restart() {
        let src = Fake::new(vec![Ok(Some(found("0.3.0")))], Ok(()));
        let (u, seen) = updater(src.clone(), true);
        let v = u.check().await;
        assert_eq!(v.phase, Phase::Ready { version: "0.3.0".into() });
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[1], Phase::Available { version: "0.3.0".into(), notes: Some("New things".into()) });
        assert!(seen.contains(&Phase::Downloading { version: "0.3.0".into(), done: 50, total: Some(100) }));
        assert_eq!(*src.installs.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn manual_mode_waits_for_install() {
        let src = Fake::new(vec![Ok(Some(found("0.3.0")))], Ok(()));
        let (u, _) = updater(src.clone(), false);
        assert!(matches!(u.check().await.phase, Phase::Available { .. }));
        assert_eq!(*src.installs.lock().unwrap(), 0);
        assert_eq!(u.install().await.phase, Phase::Ready { version: "0.3.0".into() });
    }

    #[tokio::test]
    async fn ready_skips_further_checks() {
        let src = Fake::new(vec![Ok(Some(found("0.3.0"))), Ok(Some(found("0.4.0")))], Ok(()));
        let (u, _) = updater(src.clone(), true);
        u.check().await;
        assert_eq!(u.check().await.phase, Phase::Ready { version: "0.3.0".into() });
        assert_eq!(*src.installs.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn install_without_update_does_nothing() {
        let src = Fake::new(vec![], Ok(()));
        let (u, _) = updater(src.clone(), false);
        assert_eq!(u.install().await.phase, Phase::Idle);
        assert_eq!(*src.installs.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn errors_show_and_allow_retry() {
        let src = Fake::new(vec![Err("offline".into()), Ok(Some(found("0.3.0")))], Err("bad signature".into()));
        let (u, _) = updater(src, true);
        assert_eq!(u.check().await.phase, Phase::Error { message: "Could not check for updates: offline".into() });
        assert_eq!(u.check().await.phase, Phase::Error { message: "Could not install 0.3.0: bad signature".into() });
    }

    #[test]
    fn progress_steps() {
        assert_eq!(step_of(0, Some(200)), 0);
        assert_eq!(step_of(100, Some(200)), 50);
        assert_eq!(step_of(3 * 1024 * 1024, None), 3);
    }
}
