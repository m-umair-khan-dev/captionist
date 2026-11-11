//! Model catalogue (`data/models.toml`): which speech models Captionist knows,
//! where to download them, and how accurate they are.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

const EMBEDDED: &str = include_str!("../../../data/models.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(rename = "model")]
    pub models: Vec<ModelEntry>,
}

/// Which pass a model serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Pass 1: streaming, shows draft text while someone speaks.
    Live,
    /// Pass 2: re-reads each finished sentence for the final text.
    Refine,
}

/// How the model is run by sherpa-onnx.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    OnlineTransducer,
    OfflineTransducer,
    OfflineMoonshine,
    OfflineWhisper,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelFiles {
    pub encoder: Option<String>,
    pub decoder: Option<String>,
    pub joiner: Option<String>,
    pub tokens: Option<String>,
    pub preprocessor: Option<String>,
    pub uncached_decoder: Option<String>,
    pub cached_decoder: Option<String>,
    pub merged_decoder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub engine: Engine,
    /// sherpa-onnx `model_type`, e.g. "nemo_transducer".
    pub model_type: Option<String>,
    /// Directory name after extracting the download.
    pub dir: String,
    pub url: String,
    pub download_mb: u32,
    pub files: ModelFiles,
    /// Outputs punctuation and capital letters itself.
    #[serde(default)]
    pub punctuation: bool,
    /// Seconds of silence appended when a streaming utterance ends, so the
    /// model flushes its last words.
    #[serde(default = "default_tail")]
    pub tail_padding_s: f32,
    /// Per-stream options passed to sherpa-onnx, e.g. `language = "en"`.
    #[serde(default)]
    pub stream_options: std::collections::BTreeMap<String, String>,
    /// Fallback accuracy order when measured WER is missing (higher = better).
    pub rank: u32,
    /// Word error rate (%) on LibriSpeech test-clean / test-other, measured by
    /// `captionist-bench eval`. Lower is better.
    pub wer_clean: Option<f32>,
    pub wer_other: Option<f32>,
    /// Speed (RTF) and peak RAM measured on the reference PC (Intel N150,
    /// 2 threads). Scaled by a calibration factor to estimate other PCs'
    /// speed without downloading the model.
    pub ref_rtf: Option<f32>,
    pub ref_ram_mb: Option<u64>,
    /// Small model downloaded first and timed to calibrate the estimates.
    #[serde(default)]
    pub calibration: bool,
    /// One-line description shown in the model list.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub notes: String,
}

fn default_tail() -> f32 {
    0.8
}

impl ModelEntry {
    pub fn avg_wer(&self) -> Option<f32> {
        Some((self.wer_clean? + self.wer_other?) / 2.0)
    }

    /// Share of words recognised correctly, averaged over clean and harder audio.
    pub fn accuracy_pct(&self) -> Option<f32> {
        self.avg_wer().map(|w| 100.0 - w)
    }

    /// `Less` means `self` is more accurate than `other`.
    pub fn cmp_accuracy(&self, other: &ModelEntry) -> Ordering {
        match (self.avg_wer(), other.avg_wer()) {
            (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
            _ => other.rank.cmp(&self.rank),
        }
    }

    pub fn path(&self, models_root: &Path, file: &Option<String>) -> Option<String> {
        file.as_ref().map(|f| {
            models_root
                .join(&self.dir)
                .join(f)
                .to_string_lossy()
                .to_string()
        })
    }

    pub fn is_installed(&self, models_root: &Path) -> bool {
        let f = &self.files;
        [
            &f.encoder,
            &f.decoder,
            &f.joiner,
            &f.tokens,
            &f.preprocessor,
            &f.uncached_decoder,
            &f.cached_decoder,
            &f.merged_decoder,
        ]
        .into_iter()
        .flatten()
        .all(|name| models_root.join(&self.dir).join(name).exists())
    }
}

impl Catalog {
    pub fn embedded() -> Self {
        toml::from_str(EMBEDDED).expect("embedded models.toml is valid")
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn by_role(&self, role: Role) -> impl Iterator<Item = &ModelEntry> {
        self.models.iter().filter(move |m| m.role == role)
    }

    pub fn calibration_model(&self) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.calibration)
    }
}

/// Default place for downloaded models: `$XDG_DATA_HOME/captionist/models`.
pub fn default_models_root() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("captionist/models")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses() {
        let c = Catalog::embedded();
        assert!(c.by_role(Role::Live).count() > 0);
        assert!(c.by_role(Role::Refine).count() > 0);
        let mut ids: Vec<_> = c.models.iter().map(|m| &m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.models.len(), "model ids must be unique");
        assert_eq!(
            c.models.iter().filter(|m| m.calibration).count(),
            1,
            "exactly one calibration model"
        );
        for m in &c.models {
            assert!(
                m.ref_rtf.is_some() && m.ref_ram_mb.is_some(),
                "{} needs reference speed/RAM",
                m.id
            );
            assert!(!m.summary.is_empty(), "{} needs a summary", m.id);
        }
    }
}
