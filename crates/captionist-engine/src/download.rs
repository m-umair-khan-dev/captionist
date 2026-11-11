//! Downloads and unpacks a model archive (.tar.bz2) into the models folder.

use anyhow::{Context, Result, bail};
use captionist_core::catalog::ModelEntry;
use std::fs;
use std::io::Read;
use std::path::Path;

struct Progress<R, F> {
    inner: R,
    read: u64,
    total: u64,
    last_pct: i32,
    cb: F,
}

impl<R: Read, F: FnMut(f64)> Read for Progress<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        if let Some(pct) = (self.read * 100).checked_div(self.total) {
            let pct = pct as i32;
            if pct != self.last_pct {
                self.last_pct = pct;
                (self.cb)(self.read as f64 / self.total as f64);
            }
        }
        Ok(n)
    }
}

/// Downloads `m` into `models_root/<m.dir>`. Reports progress 0.0–1.0.
/// Unpacks into a temporary folder first, so a failed download leaves nothing behind.
pub fn download(m: &ModelEntry, models_root: &Path, progress: impl FnMut(f64)) -> Result<()> {
    fs::create_dir_all(models_root)?;
    let tmp = models_root.join(format!(".download-{}", m.id));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;

    let result = (|| -> Result<()> {
        let resp = ureq::get(&m.url)
            .call()
            .with_context(|| format!("downloading {}", m.url))?;
        let total = resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(m.download_mb as u64 * 1_048_576);
        let reader = Progress {
            inner: resp.into_reader(),
            read: 0,
            total,
            last_pct: -1,
            cb: progress,
        };
        let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(reader));
        archive.unpack(&tmp).context("unpacking model")?;

        let src = tmp.join(&m.dir);
        if !src.is_dir() {
            bail!("archive did not contain {}", m.dir);
        }
        let dest = models_root.join(&m.dir);
        let _ = fs::remove_dir_all(&dest);
        fs::rename(&src, &dest)?;
        // Test clips inside archives aren't needed.
        let _ = fs::remove_dir_all(dest.join("test_wavs"));
        if !m.is_installed(models_root) {
            bail!("model files missing after download");
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

pub fn delete(m: &ModelEntry, models_root: &Path) -> Result<()> {
    let dir = models_root.join(&m.dir);
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    Ok(())
}
