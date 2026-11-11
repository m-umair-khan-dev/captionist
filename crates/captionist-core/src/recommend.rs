//! Turns hardware + measured model speeds into a recommendation, and judges a
//! manually forced choice.

use crate::catalog::{Catalog, ModelEntry, Role};
use crate::hardware::{GpuVendor, HardwareReport};
use serde::{Deserialize, Serialize};

/// Measured speed of one model on this machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedResult {
    pub model_id: String,
    pub provider: String,
    pub threads: i32,
    /// Real-time factor: compute time / audio time. 0.2 = 5x faster than real time.
    pub rtf: f32,
    pub peak_rss_mb: u64,
    pub load_ms: u64,
    /// Streaming only: 95th percentile time to process one 100 ms chunk.
    pub chunk_p95_ms: Option<f32>,
    /// Set when the model could not run (e.g. on an accelerator); `rtf` is then meaningless.
    #[serde(default)]
    pub failed: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub live_max_rtf: f32,
    pub refine_max_rtf: f32,
    /// Both models together may use at most this share of available RAM.
    pub ram_fraction: f32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            live_max_rtf: 0.35,
            refine_max_rtf: 0.5,
            ram_fraction: 0.6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    Minimal,
    Standard,
    High,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub model_id: String,
    pub model_name: String,
    pub provider: String,
    pub threads: i32,
    pub rtf: f32,
    pub peak_rss_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub tier: Tier,
    pub live: Option<Choice>,
    pub refine: Option<Choice>,
    pub notes: Vec<String>,
}

/// Typical sentence length used to turn RTF into a delay estimate.
const TYPICAL_SENTENCE_S: f32 = 6.0;

fn choice(m: &ModelEntry, r: &SpeedResult) -> Choice {
    Choice {
        model_id: m.id.clone(),
        model_name: m.name.clone(),
        provider: r.provider.clone(),
        threads: r.threads,
        rtf: r.rtf,
        peak_rss_mb: r.peak_rss_mb,
    }
}

/// Pairs of (catalogue entry, best measured result) for one role, most accurate first.
fn measured<'a>(
    catalog: &'a Catalog,
    results: &'a [SpeedResult],
    role: Role,
) -> Vec<(&'a ModelEntry, &'a SpeedResult)> {
    let mut v: Vec<_> = catalog
        .by_role(role)
        .filter_map(|m| {
            results
                .iter()
                .filter(|r| r.model_id == m.id)
                .min_by(|a, b| a.rtf.total_cmp(&b.rtf))
                .map(|r| (m, r))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp_accuracy(b.0));
    v
}

pub fn recommend(
    hw: &HardwareReport,
    catalog: &Catalog,
    results: &[SpeedResult],
    limits: Limits,
) -> Recommendation {
    let mut notes = vec![];
    let ram_budget = (hw.memory.available_mb as f32 * limits.ram_fraction) as u64;

    let lives = measured(catalog, results, Role::Live);
    let live = lives
        .iter()
        .find(|(_, r)| r.rtf <= limits.live_max_rtf && r.peak_rss_mb <= ram_budget)
        .copied()
        .or_else(|| {
            let fastest = lives.iter().min_by(|a, b| a.1.rtf.total_cmp(&b.1.rtf)).copied();
            if fastest.is_some() {
                notes.push("No live model is comfortably fast on this PC; using the fastest one. Captions may lag.".into());
            }
            fastest
        });

    let live_rss = live.map(|(_, r)| r.peak_rss_mb).unwrap_or(0);
    let refine = measured(catalog, results, Role::Refine)
        .into_iter()
        .find(|(m, r)| {
            r.rtf <= limits.refine_max_rtf
            && live_rss + r.peak_rss_mb <= ram_budget
            // Only worth running if it is more accurate than the live model.
            && live.is_none_or(|(lm, _)| m.cmp_accuracy(lm).is_lt())
        });
    if refine.is_none() {
        notes.push(
            "No correction model fits this PC's speed/memory budget; live captions only.".into(),
        );
    }

    let tier = match refine {
        None => Tier::Minimal,
        Some((_, r)) if r.provider == "cuda" => Tier::Max,
        Some((_, r)) if r.provider != "cpu" => Tier::High,
        Some(_) if hw.cpu.physical_cores >= 8 => Tier::High,
        Some(_) => Tier::Standard,
    };

    for g in &hw.gpus {
        if !g.compute_runtime
            && matches!(
                g.vendor,
                GpuVendor::Intel | GpuVendor::Nvidia | GpuVendor::Amd
            )
        {
            notes.push(format!(
                "{} could speed up correction, but its compute driver is not installed.",
                g.name
            ));
        }
    }
    if hw.on_battery {
        notes.push("Measured on battery; results may be slower than on mains power.".into());
    }

    Recommendation {
        tier,
        live: live.map(|(m, r)| choice(m, r)),
        refine: refine.map(|(m, r)| choice(m, r)),
        notes,
    }
}

/// Verdict for a manually forced model, shown before the user applies it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub ok: bool,
    /// Rough extra delay in seconds for a typical 6 s sentence.
    pub expected_delay_s: f32,
    pub message: String,
}

pub fn assess(model: &ModelEntry, result: Option<&SpeedResult>, limits: Limits) -> Verdict {
    let Some(r) = result else {
        return Verdict {
            ok: true,
            expected_delay_s: 0.0,
            message: format!("{} has not been speed-tested on this PC yet.", model.name),
        };
    };
    let limit = match model.role {
        Role::Live => limits.live_max_rtf,
        Role::Refine => limits.refine_max_rtf,
    };
    let delay = r.rtf * TYPICAL_SENTENCE_S;
    if r.rtf >= 1.0 {
        Verdict {
            ok: false,
            expected_delay_s: delay,
            message: format!(
                "{} on {}: RTF {:.2}. Slower than real time, so captions will fall further and further behind.",
                model.name, r.provider, r.rtf
            ),
        }
    } else if r.rtf > limit {
        Verdict {
            ok: false,
            expected_delay_s: delay,
            message: format!(
                "{} on {}: RTF {:.2}. Works, but captions may lag about {:.1} s and the PC will be busy.",
                model.name, r.provider, r.rtf, delay
            ),
        }
    } else {
        Verdict {
            ok: true,
            expected_delay_s: delay,
            message: format!(
                "{} on {}: RTF {:.2}. Runs comfortably.",
                model.name, r.provider, r.rtf
            ),
        }
    }
}
