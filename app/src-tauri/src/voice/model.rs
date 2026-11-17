//! The on-device speech model: Moonshine base, int8 ONNX (about 64 MB). Downloaded once
//! into ~/.bleep/models and checked by SHA-256. A copy that WhisprCatch already
//! downloaded is used in place, so nothing is fetched twice.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct FileSpec {
    pub name: &'static str,
    pub url_path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub const DIR_NAME: &str = "moonshine-base-int8";
pub const BASE_URL: &str = "https://huggingface.co/onnx-community/moonshine-base-ONNX/resolve/main";
pub const FILES: &[FileSpec] = &[
    FileSpec { name: "encoder_model.int8.onnx", url_path: "onnx/encoder_model_int8.onnx", size: 20_488_801, sha256: "4edb38db96b52ddee5a3b25d2211bb6394f9a9a2d95f67bce0e8c861da018a4d" },
    FileSpec { name: "decoder_model_merged.int8.onnx", url_path: "onnx/decoder_model_merged_int8.onnx", size: 42_427_261, sha256: "45febab0347ffbe1326459e092ae20a2cfda10ca33ab018b3c299247284de61b" },
    FileSpec { name: "tokenizer.json", url_path: "tokenizer.json", size: 3_761_754, sha256: "7b913404bdd039af4756783218af4440bc07fb7d6d8258d677e34f95b3ec416f" },
];

pub fn total_size() -> u64 {
    FILES.iter().map(|f| f.size).sum()
}

/// Every file is there with the right size. (The hash is checked when downloading.)
pub fn is_complete(dir: &Path) -> bool {
    FILES.iter().all(|f| dir.join(f.name).metadata().map(|m| m.len()).ok() == Some(f.size))
}

/// Where bleep keeps its own copy.
pub fn own_dir(models_root: &Path) -> PathBuf {
    models_root.join(DIR_NAME)
}

/// WhisprCatch's copy, if that app is installed.
pub fn whisprcatch_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("whisper-catch").join("models").join(DIR_NAME))
}

/// A complete model folder, if there is one: bleep's own first, then WhisprCatch's.
pub fn find(models_root: &Path, others: &[PathBuf]) -> Option<PathBuf> {
    std::iter::once(own_dir(models_root)).chain(others.iter().cloned()).find(|d| is_complete(d))
}

/// Downloads what is missing into bleep's own folder. `progress(done, total)` in bytes.
pub async fn download(http: &reqwest::Client, models_root: &Path, progress: &(dyn Fn(u64, u64) + Send + Sync)) -> Result<PathBuf> {
    let dir = own_dir(models_root);
    std::fs::create_dir_all(&dir)?;
    let total = total_size();
    let mut done = 0;
    for f in FILES {
        let dest = dir.join(f.name);
        if dest.metadata().map(|m| m.len()).ok() == Some(f.size) {
            done += f.size;
            progress(done, total);
            continue;
        }
        let url = format!("{BASE_URL}/{}", f.url_path);
        let mut resp = http.get(&url).send().await.and_then(|r| r.error_for_status()).with_context(|| format!("download {}", f.name))?;
        let part = dir.join(format!("{}.part", f.name));
        let mut out = tokio::fs::File::create(&part).await?;
        let mut hash = Sha256::new();
        let mut got = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            hash.update(&chunk);
            got += chunk.len() as u64;
            tokio::io::AsyncWriteExt::write_all(&mut out, &chunk).await?;
            progress(done + got, total);
        }
        tokio::io::AsyncWriteExt::flush(&mut out).await?;
        drop(out);
        let sum = hex::encode(hash.finalize());
        if got != f.size || sum != f.sha256 {
            let _ = std::fs::remove_file(&part);
            bail!("{} did not match its checksum. Try again.", f.name);
        }
        std::fs::rename(&part, &dest)?;
        done += f.size;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_model(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for f in FILES {
            let file = std::fs::File::create(dir.join(f.name)).unwrap();
            file.set_len(f.size).unwrap();
        }
    }

    #[test]
    fn find_prefers_own_copy_then_others() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let other_dir = other.path().join(DIR_NAME);
        assert_eq!(find(root.path(), std::slice::from_ref(&other_dir)), None);
        fake_model(&other_dir);
        assert_eq!(find(root.path(), std::slice::from_ref(&other_dir)), Some(other_dir.clone()));
        fake_model(&own_dir(root.path()));
        assert_eq!(find(root.path(), &[other_dir]), Some(own_dir(root.path())));
    }

    #[test]
    fn a_short_file_is_not_complete() {
        let root = tempfile::tempdir().unwrap();
        let d = own_dir(root.path());
        fake_model(&d);
        std::fs::File::options().write(true).open(d.join(FILES[0].name)).unwrap().set_len(10).unwrap();
        assert!(!is_complete(&d));
    }

    #[test]
    fn spec_is_about_64_mb() {
        assert_eq!(total_size() / 1_000_000, 66);
    }
}
