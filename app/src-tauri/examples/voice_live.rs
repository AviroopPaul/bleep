//! Live check of the wake word on the real mic: plays "hey bleep" and a request through
//! the speakers with `say`, and prints what the listener does.
//! `cargo run --example voice_live -- [seconds] [phrase to speak]`
use bleep_lib::voice::{capture, listener::{max_gap, Listener}, model, moonshine::Moonshine, vad::Vad};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(14);
    let speak = args.get(2).cloned();
    let root = bleep_lib::paths::BleepPaths::from_env().root.join("models");
    let dir = model::find(&root, &model::whisprcatch_dir().into_iter().collect::<Vec<_>>()).expect("speech model on disk");
    let mut stt = Moonshine::load(&dir)?;
    let name = capture::pick("", &capture::input_devices(), capture::default_input().as_deref()).expect("a mic");
    let (tx, rx) = capture::channel();
    let mic = capture::Mic::open(&name, tx)?;
    println!("mic: {} (inputs: {:?})", mic.name, capture::input_devices());
    if let Some(text) = speak {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            for part in text.split('|') {
                println!("SAY  {part}");
                let v = std::env::var("VOICE").unwrap_or_default();
                let mut c = std::process::Command::new("say");
                if !v.is_empty() {
                    c.args(["-v", &v]);
                }
                let _ = c.arg(part).status();
                std::thread::sleep(Duration::from_millis(900));
            }
        });
    }
    let (mut vad, mut l) = (Vad::new(), Listener::new("hey bleep", max_gap("normal")));
    let start = Instant::now();
    let mut peak = 0f32;
    while start.elapsed() < Duration::from_secs(secs) {
        let Ok(chunk) = rx.recv_timeout(Duration::from_millis(250)) else { continue };
        peak = peak.max(bleep_lib::voice::vad::rms(&chunk));
        vad.set_pause_ms(l.pause_ms());
        for ev in vad.push(&chunk) {
            if let bleep_lib::voice::vad::VadEvent::End { audio, .. } = &ev {
                let s = stt.spot(audio, "hey bleep", -1.0)?;
                println!("UTTER {:.2}s gap {:.2} heard {:?}", audio.len() as f32 / 16000.0, s.gap, s.heard);
            }
            let t = Instant::now();
            let acts = l.on_event(ev, &mut stt);
            println!("HEARD {:?} -> {:?} ({:?})", l.last_heard, acts, t.elapsed());
        }
        if let Some(a) = l.tick(vad.now()) {
            println!("TICK {a:?}");
        }
    }
    println!("peak level {peak:.4}");
    Ok(())
}
