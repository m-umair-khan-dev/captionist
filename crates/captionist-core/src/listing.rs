//! The model list shown in Settings → Engine & Models: every catalogue model
//! with its accuracy, its speed on this PC (measured, or estimated without
//! downloading it), how well it fits, and sorting.

use crate::catalog::{Catalog, ModelEntry, Role};
use crate::recommend::{Limits, Recommendation, SpeedResult};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeedSource {
    /// Timed on this PC.
    Measured,
    /// Reference speed scaled by this PC's calibration factor.
    Estimated,
    Unknown,
}

/// How well a model runs on this PC. The UI colours it: green / amber / red.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Fit {
    Smooth,
    MayLag,
    Unknown,
    TooSlow,
}

impl Fit {
    pub fn label(self) -> &'static str {
        match self {
            Fit::Smooth => "Runs smoothly",
            Fit::MayLag => "May lag",
            Fit::Unknown => "Not tested",
            Fit::TooSlow => "Too slow for this PC",
        }
    }

    /// libadwaita style class for the label colour.
    pub fn css_class(self) -> &'static str {
        match self {
            Fit::Smooth => "success",
            Fit::MayLag => "warning",
            Fit::Unknown => "dim-label",
            Fit::TooSlow => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRow {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub role: Role,
    /// Share of words recognised correctly (%).
    pub accuracy_pct: Option<f32>,
    pub rtf: Option<f32>,
    pub speed_source: SpeedSource,
    pub ram_mb: Option<u64>,
    pub download_mb: u32,
    pub installed: bool,
    pub recommended: bool,
    pub fit: Fit,
    /// Plain-language warning for amber/red models (empty when it runs smoothly).
    pub fit_message: String,
}

/// Everything the Settings window needs for the Engine & Models page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsReport {
    pub hardware: crate::hardware::HardwareReport,
    /// > 1 = slower than the reference PC.
    pub factor: Option<f32>,
    /// Long-running job in progress, e.g. "Checking this PC…".
    pub busy: Option<String>,
    pub recommendation: Recommendation,
    pub rows: Vec<ModelRow>,
    pub active_live: Option<String>,
    pub active_refine: Option<String>,
    /// (model id, progress 0–1)
    pub downloading: Vec<(String, f64)>,
    /// Accelerators found on this PC, with advice (see `accel::scan`).
    pub accel: Vec<crate::accel::AccelOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SortBy {
    /// Recommended first, then models that run well, most accurate first.
    #[default]
    Recommended,
    Accuracy,
    Speed,
    Download,
    Memory,
}

impl SortBy {
    pub const ALL: [SortBy; 5] = [
        SortBy::Recommended,
        SortBy::Accuracy,
        SortBy::Speed,
        SortBy::Download,
        SortBy::Memory,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortBy::Recommended => "Recommended",
            SortBy::Accuracy => "Most accurate",
            SortBy::Speed => "Fastest",
            SortBy::Download => "Smallest download",
            SortBy::Memory => "Least memory",
        }
    }
}

impl FromStr for SortBy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "recommended" => Ok(SortBy::Recommended),
            "accuracy" | "quality" => Ok(SortBy::Accuracy),
            "speed" | "fastest" => Ok(SortBy::Speed),
            "download" | "size" => Ok(SortBy::Download),
            "memory" | "ram" => Ok(SortBy::Memory),
            _ => Err(format!(
                "unknown sort '{s}' (recommended, accuracy, speed, download, memory)"
            )),
        }
    }
}

/// How much slower (>1) or faster (<1) this PC is than the reference PC,
/// from the CPU models timed here. Median, so one odd result doesn't skew it.
pub fn calibration_factor(catalog: &Catalog, measured: &[SpeedResult]) -> Option<f32> {
    let mut ratios: Vec<f32> = measured
        .iter()
        .filter(|r| r.provider == "cpu")
        .filter_map(|r| Some(r.rtf / catalog.get(&r.model_id)?.ref_rtf?))
        .collect();
    if ratios.is_empty() {
        return None;
    }
    ratios.sort_by(f32::total_cmp);
    Some(ratios[ratios.len() / 2])
}

/// Measured results, plus estimates for every model not timed on this PC.
pub fn effective_speeds(
    catalog: &Catalog,
    measured: &[SpeedResult],
    factor: Option<f32>,
    threads: i32,
) -> Vec<(SpeedResult, SpeedSource)> {
    let mut out: Vec<_> = measured
        .iter()
        .map(|r| (r.clone(), SpeedSource::Measured))
        .collect();
    if let Some(f) = factor {
        for m in &catalog.models {
            if measured.iter().any(|r| r.model_id == m.id) {
                continue;
            }
            if let Some(ref_rtf) = m.ref_rtf {
                out.push((
                    SpeedResult {
                        model_id: m.id.clone(),
                        provider: "cpu".into(),
                        threads,
                        rtf: ref_rtf * f,
                        peak_rss_mb: m.ref_ram_mb.unwrap_or(0),
                        load_ms: 0,
                        chunk_p95_ms: None,
                        failed: None,
                    },
                    SpeedSource::Estimated,
                ));
            }
        }
    }
    out
}

pub fn fit(role: Role, rtf: Option<f32>, limits: Limits) -> Fit {
    let Some(rtf) = rtf else { return Fit::Unknown };
    let limit = match role {
        Role::Live => limits.live_max_rtf,
        Role::Refine => limits.refine_max_rtf,
    };
    if rtf <= limit {
        Fit::Smooth
    } else if rtf < 1.0 {
        Fit::MayLag
    } else {
        Fit::TooSlow
    }
}

pub fn rows(
    catalog: &Catalog,
    speeds: &[(SpeedResult, SpeedSource)],
    rec: &Recommendation,
    is_installed: impl Fn(&ModelEntry) -> bool,
    limits: Limits,
) -> Vec<ModelRow> {
    catalog
        .models
        .iter()
        .map(|m| {
            // Prefer a measured result; among several, the fastest provider.
            let best = speeds
                .iter()
                .filter(|(r, _)| r.model_id == m.id)
                .min_by(|a, b| (a.1 != SpeedSource::Measured).cmp(&(b.1 != SpeedSource::Measured)).then(a.0.rtf.total_cmp(&b.0.rtf)));
            let (rtf, ram, source) = match best {
                Some((r, s)) => (Some(r.rtf), Some(r.peak_rss_mb), *s),
                None => (None, m.ref_ram_mb, SpeedSource::Unknown),
            };
            let recommended = [&rec.live, &rec.refine].into_iter().flatten().any(|c| c.model_id == m.id);
            let fit = fit(m.role, rtf, limits);
            let fit_message = match (fit, rtf) {
                (Fit::MayLag, Some(r)) => format!("Captions may lag about {:.1} s on this PC, and it will keep the CPU busy.", r * 6.0),
                (Fit::TooSlow, _) => "Slower than real time on this PC: captions will fall further and further behind.".into(),
                _ => String::new(),
            };
            ModelRow {
                id: m.id.clone(),
                name: m.name.clone(),
                summary: m.summary.clone(),
                role: m.role,
                accuracy_pct: m.accuracy_pct(),
                rtf,
                speed_source: source,
                ram_mb: ram,
                download_mb: m.download_mb,
                installed: is_installed(m),
                recommended,
                fit,
                fit_message,
            }
        })
        .collect()
}

fn cmp_opt_f32(a: Option<f32>, b: Option<f32>) -> Ordering {
    // `None` sorts last.
    match (a, b) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn sort_rows(rows: &mut [ModelRow], by: SortBy) {
    let acc_desc = |a: &ModelRow, b: &ModelRow| cmp_opt_f32(b.accuracy_pct, a.accuracy_pct);
    rows.sort_by(|a, b| match by {
        SortBy::Recommended => b
            .recommended
            .cmp(&a.recommended)
            .then(a.fit.cmp(&b.fit))
            .then(acc_desc(a, b)),
        SortBy::Accuracy => cmp_opt_f32(b.accuracy_pct, a.accuracy_pct).then(a.fit.cmp(&b.fit)),
        SortBy::Speed => cmp_opt_f32(a.rtf, b.rtf),
        SortBy::Download => a.download_mb.cmp(&b.download_mb).then(acc_desc(a, b)),
        SortBy::Memory => a
            .ram_mb
            .unwrap_or(u64::MAX)
            .cmp(&b.ram_mb.unwrap_or(u64::MAX))
            .then(acc_desc(a, b)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::HardwareReport;
    use crate::recommend::recommend;

    fn measured(id: &str, rtf: f32) -> SpeedResult {
        SpeedResult {
            model_id: id.into(),
            provider: "cpu".into(),
            threads: 2,
            rtf,
            peak_rss_mb: 200,
            load_ms: 0,
            chunk_p95_ms: None,
            failed: None,
        }
    }

    #[test]
    fn estimates_scale_with_calibration() {
        let c = Catalog::embedded();
        let cal = c.calibration_model().expect("a calibration model");
        // This PC is twice as slow as the reference PC.
        let m = vec![measured(&cal.id, cal.ref_rtf.unwrap() * 2.0)];
        let f = calibration_factor(&c, &m).unwrap();
        assert!((f - 2.0).abs() < 1e-4);
        let speeds = effective_speeds(&c, &m, Some(f), 2);
        assert_eq!(speeds.len(), c.models.len(), "every model gets a speed");
        let p = c.get("parakeet-tdt-0.6b-v2").unwrap();
        let est = speeds.iter().find(|(r, _)| r.model_id == p.id).unwrap();
        assert_eq!(est.1, SpeedSource::Estimated);
        assert!((est.0.rtf - p.ref_rtf.unwrap() * 2.0).abs() < 1e-4);
    }

    #[test]
    fn reference_pc_recommends_kroko_plus_parakeet() {
        let c = Catalog::embedded();
        let cal = c.calibration_model().unwrap();
        let m = vec![measured(&cal.id, cal.ref_rtf.unwrap())];
        let speeds = effective_speeds(&c, &m, calibration_factor(&c, &m), 2);
        let only: Vec<_> = speeds.iter().map(|(r, _)| r.clone()).collect();
        let mut hw = HardwareReport::probe();
        hw.memory.available_mb = 4000;
        let rec = recommend(&hw, &c, &only, Limits::default());
        assert_eq!(rec.live.as_ref().unwrap().model_id, "kroko-en");
        assert_eq!(
            rec.refine.as_ref().unwrap().model_id,
            "parakeet-tdt-0.6b-v2"
        );

        let mut r = rows(&c, &speeds, &rec, |_| false, Limits::default());
        sort_rows(&mut r, SortBy::Recommended);
        assert!(r[0].recommended && r[1].recommended);
        sort_rows(&mut r, SortBy::Speed);
        assert!(r.windows(2).all(|w| w[0].rtf <= w[1].rtf));
    }
}
