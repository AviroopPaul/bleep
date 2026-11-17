//! Voice: "hey bleep" wakes the droid and opens the chat, the next thing said is sent,
//! and the reply can be read aloud. Speech to text runs on the Mac (Moonshine).
//! Nothing is recorded or kept: audio lives in memory for one utterance.
pub mod capture;
pub mod listener;
pub mod model;
pub mod moonshine;
pub mod stt;
pub mod tts;
pub mod vad;
pub mod wake;

use crate::config::VoiceConfig;
use crate::AppState;
use listener::{Action, Listener};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Off,
    Downloading,
    Loading,
    Listening,
    Awake,
    Error,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Status {
    pub phase: Phase,
    /// A short line for Settings: an error, or a hint.
    pub detail: String,
    /// The mic in use.
    pub mic: Option<String>,
    /// Recent input level, 0 to 1.
    pub level: f32,
    pub last_heard: Option<String>,
    /// Download progress, 0 to 1.
    pub progress: Option<f32>,
}

pub type ConfigFn = Arc<dyn Fn() -> VoiceConfig + Send + Sync>;
pub type ActionFn = Arc<dyn Fn(Action) + Send + Sync>;

pub const NO_SOUND: &str = "No sound from the mic. Allow bleep in System Settings, Privacy and Security, Microphone.";

/// Owns the listening thread and the speaker.
pub struct Voice {
    status: Arc<Mutex<Status>>,
    running: Arc<AtomicBool>,
    pub speaker: tts::Speaker,
    models_root: PathBuf,
}

impl Voice {
    pub fn new(models_root: PathBuf) -> Self {
        Self { status: Default::default(), running: Default::default(), speaker: Default::default(), models_root }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn model_ready(&self) -> bool {
        model::find(&self.models_root, &others()).is_some()
    }

    /// Starts the listener when the config says so. Safe to call again: one thread at most.
    pub fn sync(&self, cfg: ConfigFn, on: ActionFn) {
        if !cfg().wake || self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let (status, running, speaker, root) = (self.status.clone(), self.running.clone(), self.speaker.clone(), self.models_root.clone());
        std::thread::Builder::new()
            .name("bleep-voice".into())
            .spawn(move || loop {
                match listen(&cfg, &on, &status, &speaker, &root) {
                    Err(e) => {
                        eprintln!("bleep: voice: {e:#}");
                        let mut s = status.lock().unwrap();
                        s.phase = Phase::Error;
                        s.detail = e.to_string();
                        drop(s);
                        running.store(false, Ordering::SeqCst);
                        return;
                    }
                    Ok(()) => {
                        let last_heard = status.lock().unwrap().last_heard.clone();
                        *status.lock().unwrap() = Status { last_heard, ..Default::default() };
                    }
                }
                running.store(false, Ordering::SeqCst);
                // Turned back on while this thread was stopping: carry on here.
                if !cfg().wake || running.swap(true, Ordering::SeqCst) {
                    return;
                }
            })
            .expect("voice thread");
    }
}

fn others() -> Vec<PathBuf> {
    model::whisprcatch_dir().into_iter().collect()
}

fn set(status: &Mutex<Status>, f: impl FnOnce(&mut Status)) {
    f(&mut status.lock().unwrap());
}

fn load_model(root: &PathBuf, status: &Mutex<Status>) -> anyhow::Result<moonshine::Moonshine> {
    let dir = match model::find(root, &others()) {
        Some(d) => d,
        None => {
            set(status, |s| {
                s.phase = Phase::Downloading;
                s.detail = format!("Downloading the speech model ({} MB, once).", model::total_size() / 1_000_000);
            });
            let http = reqwest::Client::builder().timeout(Duration::from_secs(600)).build()?;
            tauri::async_runtime::block_on(model::download(&http, root, &|done, total| set(status, |s| s.progress = Some(done as f32 / total as f32))))?
        }
    };
    set(status, |s| {
        s.phase = Phase::Loading;
        s.progress = None;
        s.detail = String::new();
    });
    moonshine::Moonshine::load(&dir)
}

/// The listening loop. Returns when the wake word is turned off.
fn listen(cfg: &ConfigFn, on: &ActionFn, status: &Mutex<Status>, speaker: &tts::Speaker, root: &PathBuf) -> anyhow::Result<()> {
    let mut stt = load_model(root, status)?;
    let mut c = cfg();
    let mut listener = Listener::new(&c.phrase, listener::max_gap(&c.sensitivity));
    loop {
        if !c.wake {
            return Ok(());
        }
        let want = capture::pick(&c.mic, &capture::input_devices(), capture::default_input().as_deref()).ok_or_else(|| anyhow::anyhow!("No microphone found."))?;
        let (tx, rx) = capture::channel();
        let mic = capture::Mic::open(&want, tx)?;
        eprintln!("bleep: listening on {}", mic.name);
        set(status, |s| {
            s.phase = Phase::Listening;
            s.mic = Some(mic.name.clone());
            s.detail = String::new();
        });
        let mut vad = vad::Vad::new();
        let mut checked = Instant::now();
        let mut silent_since = Instant::now();
        listener.reset();
        loop {
            let chunk = match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(c) => c,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => vec![],
                Err(_) => break,
            };
            let level = vad::rms(&chunk);
            if level > 0.0 {
                silent_since = Instant::now();
            }
            let hint = if silent_since.elapsed() > Duration::from_secs(3) { NO_SOUND } else { "" };
            set(status, |s| {
                s.level = s.level * 0.6 + level * 0.4;
                s.detail = hint.into();
            });
            if speaker.is_speaking() {
                // Do not hear bleep's own voice as a request.
                vad.reset();
                continue;
            }
            vad.set_pause_ms(listener.pause_ms());
            for ev in vad.push(&chunk) {
                for a in listener.on_event(ev, &mut stt) {
                    act(a, on, status, speaker);
                }
            }
            if let Some(a) = listener.tick(vad.now()) {
                act(a, on, status, speaker);
            }
            if let Some(h) = listener.last_heard.clone() {
                set(status, |s| s.last_heard = Some(h));
            }
            // Once a second: settings changed, or the mic to use did (AirPods in or out).
            if checked.elapsed() > Duration::from_secs(1) {
                checked = Instant::now();
                let new = cfg();
                if new.phrase != c.phrase || new.sensitivity != c.sensitivity {
                    listener.set_phrase(&new.phrase, listener::max_gap(&new.sensitivity));
                }
                let again = !new.wake || new.mic != c.mic || capture::pick(&new.mic, &capture::input_devices(), capture::default_input().as_deref()).as_deref() != Some(mic.name.as_str());
                c = new;
                if again && !listener.awake() {
                    break;
                }
            }
        }
        drop(mic);
    }
}

fn act(a: Action, on: &ActionFn, status: &Mutex<Status>, speaker: &tts::Speaker) {
    match &a {
        Action::Wake => {
            speaker.stop();
            set(status, |s| s.phase = Phase::Awake);
        }
        Action::Heard(_) | Action::GaveUp => set(status, |s| s.phase = Phase::Listening),
    }
    on(a);
}

/// What the chat window and the droid hear about.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", content = "text", rename_all = "lowercase")]
pub enum VoiceEvent {
    Wake,
    Heard(String),
    Idle,
}

fn config_fn(app: &AppHandle) -> ConfigFn {
    let h = app.clone();
    Arc::new(move || h.state::<AppState>().config.lock().unwrap().voice.clone())
}

fn action_fn(app: &AppHandle) -> ActionFn {
    let h = app.clone();
    Arc::new(move |a: Action| {
        let ev = match a {
            Action::Wake => {
                let a = h.clone();
                let _ = h.run_on_main_thread(move || crate::windows::open_chat(&a, crate::windows::Anchor::Droid));
                VoiceEvent::Wake
            }
            Action::Heard(t) => VoiceEvent::Heard(t),
            Action::GaveUp => VoiceEvent::Idle,
        };
        let _ = h.emit("voice://event", ev);
    })
}

pub fn start(app: &AppHandle) {
    app.state::<Arc<Voice>>().sync(config_fn(app), action_fn(app));
}

#[derive(Serialize)]
pub struct VoiceView {
    pub config: VoiceConfig,
    pub status: Status,
    pub model_ready: bool,
    pub model_mb: u64,
    pub mics: Vec<String>,
    pub default_mic: Option<String>,
    pub speakers: Vec<tts::OutputDevice>,
    pub voices: Vec<tts::Voice>,
}

fn view(app: &AppHandle) -> VoiceView {
    let v = app.state::<Arc<Voice>>();
    VoiceView {
        config: app.state::<AppState>().config.lock().unwrap().voice.clone(),
        status: v.status(),
        model_ready: v.model_ready(),
        model_mb: model::total_size() / 1_000_000,
        mics: capture::input_devices(),
        default_mic: capture::default_input(),
        speakers: tts::output_devices(),
        voices: tts::voices(),
    }
}

#[tauri::command]
pub async fn voice_settings(app: AppHandle) -> VoiceView {
    tauri::async_runtime::spawn_blocking(move || view(&app)).await.expect("voice view")
}

#[tauri::command]
pub fn voice_status(voice: tauri::State<'_, Arc<Voice>>) -> Status {
    voice.status()
}

/// Saves the voice settings. The listener picks up a new phrase or mic within a second.
#[tauri::command]
pub async fn voice_set(app: AppHandle, config: VoiceConfig) -> Result<VoiceView, String> {
    let phrase = wake::validate(&config.phrase)?;
    {
        let st = app.state::<AppState>();
        let mut cfg = st.config.lock().unwrap();
        let sensitivity = if ["strict", "normal", "loose"].contains(&config.sensitivity.as_str()) { config.sensitivity.clone() } else { "normal".into() };
        cfg.voice = VoiceConfig { phrase, sensitivity, ..config };
        cfg.save(&st.paths.config_file).map_err(|e| e.to_string())?;
    }
    if !app.state::<AppState>().config.lock().unwrap().voice.speak {
        app.state::<Arc<Voice>>().speaker.stop();
    }
    start(&app);
    Ok(tauri::async_runtime::spawn_blocking(move || view(&app)).await.map_err(|e| e.to_string())?)
}

/// Reads a reply aloud, if spoken replies are on.
#[tauri::command]
pub fn voice_speak(app: AppHandle, text: String) -> Result<(), String> {
    let c = app.state::<AppState>().config.lock().unwrap().voice.clone();
    if !c.speak {
        return Ok(());
    }
    app.state::<Arc<Voice>>().speaker.speak(&text, &c.voice, &c.speaker).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn voice_stop_speaking(voice: tauri::State<'_, Arc<Voice>>) {
    voice.speaker.stop();
}

/// A sample line with the chosen voice and speaker, whether spoken replies are on or not.
#[tauri::command]
pub fn voice_test_speaker(app: AppHandle, voice: String, speaker: String) -> Result<(), String> {
    app.state::<Arc<Voice>>().speaker.speak("Hi, I am bleep. This is how I sound.", &voice, &speaker).map_err(|e| e.to_string())
}
