//! Times a model on the bundled calibration clip (LibriSpeech, ~45 s).

use crate::asr::{self, SAMPLE_RATE};
use anyhow::{Context, Result, bail};
use captionist_core::catalog::ModelEntry;
use captionist_core::recommend::SpeedResult;
use std::path::Path;
use std::time::Instant;

/// Clip is cut into sentence-sized pieces, like real captioning.
const PIECE_S: usize = 8;

pub fn load_clip(path: &Path) -> Result<Vec<f32>> {
    let wave = sherpa_onnx::Wave::read(&path.to_string_lossy())
        .with_context(|| format!("reading {}", path.display()))?;
    if wave.sample_rate() != SAMPLE_RATE {
        bail!("{} must be 16 kHz", path.display());
    }
    Ok(wave.samples().to_vec())
}

fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

/// Measures one model's speed on this PC.
/// `provider` goes to sherpa-onnx; `label` is stored in the result ("cpu", "openvino-gpu").
pub fn measure(
    m: &ModelEntry,
    models_root: &Path,
    clip: &[f32],
    threads: i32,
    provider: &str,
    label: &str,
) -> Result<SpeedResult> {
    let rss_before = peak_rss_mb();
    let t = Instant::now();
    let rec = asr::build(m, models_root, threads, provider)?;
    let load_ms = t.elapsed().as_millis() as u64;
    rec.transcribe(m, &vec![0.0; SAMPLE_RATE as usize])?; // warm-up

    let t = Instant::now();
    for piece in clip.chunks(PIECE_S * SAMPLE_RATE as usize) {
        rec.transcribe(m, piece)?;
    }
    let compute = t.elapsed().as_secs_f32();
    let audio = clip.len() as f32 / SAMPLE_RATE as f32;
    let ram = peak_rss_mb()
        .saturating_sub(rss_before)
        .max(m.ref_ram_mb.map(|r| r / 2).unwrap_or(0));
    Ok(SpeedResult {
        model_id: m.id.clone(),
        provider: label.into(),
        threads,
        rtf: compute / audio,
        peak_rss_mb: ram,
        load_ms,
        chunk_p95_ms: None,
        failed: None,
    })
}
