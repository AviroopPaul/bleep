//! Every place bleep opens a URL goes through this, so tests never open a browser.
pub trait Opener: Send + Sync {
    fn open(&self, url: &str) -> anyhow::Result<()>;
}

/// Opens in the default browser through the Tauri opener plugin.
pub struct SystemOpener(pub tauri::AppHandle);

impl Opener for SystemOpener {
    fn open(&self, url: &str) -> anyhow::Result<()> {
        use tauri_plugin_opener::OpenerExt;
        self.0.opener().open_url(url, None::<&str>)?;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct RecordingOpener {
    pub urls: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl Opener for RecordingOpener {
    fn open(&self, url: &str) -> anyhow::Result<()> {
        self.urls.lock().unwrap().push(url.to_string());
        Ok(())
    }
}
