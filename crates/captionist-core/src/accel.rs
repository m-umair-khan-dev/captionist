//! Optional hardware acceleration: which accelerators this PC has, whether
//! they're ready, and whether to recommend them.
//!
//! Captionist's sherpa-onnx is built against ONNX Runtime 1.24.1. Each
//! accelerator is served by a "pack": a vendor build of that same runtime
//! version with the matching execution provider, downloaded on request.

use crate::hardware::{GpuVendor, HardwareReport};
use crate::recommend::SpeedResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Intel integrated graphics or Arc, via OpenVINO.
    IntelGpu,
    /// Intel NPU (Core Ultra), via OpenVINO.
    IntelNpu,
    /// NVIDIA GPU, via CUDA.
    NvidiaGpu,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::NvidiaGpu, Kind::IntelGpu, Kind::IntelNpu];

    /// Value of the `accelerator` setting.
    pub fn id(self) -> &'static str {
        match self {
            Kind::IntelGpu => "intel-gpu",
            Kind::IntelNpu => "intel-npu",
            Kind::NvidiaGpu => "nvidia-gpu",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::IntelGpu => "Intel graphics (OpenVINO)",
            Kind::IntelNpu => "Intel NPU (OpenVINO)",
            Kind::NvidiaGpu => "NVIDIA GPU (CUDA)",
        }
    }
}

/// A downloadable runtime pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pack {
    /// Folder name under `~/.local/share/captionist/accel/`.
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub download_mb: u32,
    /// Path prefix of the libraries inside the archive.
    pub lib_prefix: &'static str,
    pub archive: Archive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    /// Python wheel (zip).
    Wheel,
    /// .tgz
    TarGz,
}

pub const OPENVINO: Pack = Pack {
    id: "openvino",
    name: "Intel OpenVINO runtime",
    url: "https://files.pythonhosted.org/packages/50/cf/17ba72de2df0fcba349937d2788f154397bbc2d1a2d67772a97e26f6bc5f/onnxruntime_openvino-1.24.1-cp312-cp312-manylinux_2_28_x86_64.whl",
    sha256: "d617fac2f59a6ab5ea59a788c3e1592240a129642519aaeaa774761dfe35150e",
    download_mb: 81,
    lib_prefix: "onnxruntime/capi/",
    archive: Archive::Wheel,
};

pub const CUDA12: Pack = Pack {
    id: "cuda12",
    name: "ONNX Runtime CUDA 12",
    url: "https://github.com/microsoft/onnxruntime/releases/download/v1.24.1/onnxruntime-linux-x64-gpu-1.24.1.tgz",
    sha256: "1c468821456b7863640555e31ee5b71e56bb959874b9db0dbf79503997993673",
    download_mb: 195,
    lib_prefix: "onnxruntime-linux-x64-gpu-1.24.1/lib/",
    archive: Archive::TarGz,
};

pub const CUDA13: Pack = Pack {
    id: "cuda13",
    name: "ONNX Runtime CUDA 13",
    url: "https://github.com/microsoft/onnxruntime/releases/download/v1.24.1/onnxruntime-linux-x64-gpu_cuda13-1.24.1.tgz",
    sha256: "30aaf9b1046aab7ed976dca09ca64c11da5385871a0d2b9aa00e2817f2416f1e",
    download_mb: 171,
    lib_prefix: "onnxruntime-linux-x64-gpu_cuda13-1.24.1/lib/",
    archive: Archive::TarGz,
};

/// Advice shown next to an accelerator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Advice {
    /// Expected to help a lot (dedicated GPU). Not measured yet.
    Recommended,
    /// Might help (built-in graphics, NPU): mainly frees the processor. Not measured yet.
    WorthTesting,
    /// Measured faster than the processor.
    Faster,
    /// Measured about the same or slower: not worth it.
    NotWorthIt,
    /// A driver or runtime library must be installed first.
    NeedsDriver,
    /// Hardware found but Captionist can't use it (yet).
    Unsupported,
}

impl Advice {
    pub fn label(self) -> &'static str {
        match self {
            Advice::Recommended => "Recommended",
            Advice::WorthTesting => "Worth testing",
            Advice::Faster => "Faster",
            Advice::NotWorthIt => "Not worth it",
            Advice::NeedsDriver => "Driver needed",
            Advice::Unsupported => "Not supported",
        }
    }

    /// libadwaita colour class.
    pub fn css_class(self) -> &'static str {
        match self {
            Advice::Recommended | Advice::Faster => "success",
            Advice::WorthTesting | Advice::NeedsDriver => "warning",
            Advice::NotWorthIt | Advice::Unsupported => "dim-label",
        }
    }
}

/// One accelerator found on this PC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccelOption {
    /// None for hardware Captionist can't use (e.g. AMD).
    pub kind: Option<Kind>,
    pub device: String,
    pub advice: Advice,
    /// One-line explanation, e.g. the install command for a missing driver.
    pub message: String,
    pub driver_ok: bool,
    /// Runtime pack needed, if any: (id, name, download MB).
    pub pack: Option<(String, String, u32)>,
    pub pack_installed: bool,
    /// The `accelerator` setting selects this.
    pub enabled: bool,
    /// The caption service is running on it right now.
    pub active: bool,
    /// Measured (model, processor RTF, accelerator RTF or None if it failed) for correction models.
    pub results: Vec<(String, Option<f32>, Option<f32>)>,
}

/// Which CUDA pack fits the installed CUDA libraries.
pub fn cuda_pack(hw: &HardwareReport) -> Option<Pack> {
    match hw.cuda_major {
        Some(13) => Some(CUDA13),
        Some(12) => Some(CUDA12),
        _ => None,
    }
}

pub fn pack_for(kind: Kind, hw: &HardwareReport) -> Option<Pack> {
    match kind {
        Kind::IntelGpu | Kind::IntelNpu => Some(OPENVINO),
        Kind::NvidiaGpu => cuda_pack(hw),
    }
}

/// Result label stored with speed tests on this accelerator.
pub fn result_label(kind: Kind) -> String {
    format!("accel:{}", kind.id())
}

/// Scans the hardware and turns it into advice.
///
/// `measured` holds speed tests ("cpu" and "accel:<kind>" labels),
/// `pack_installed` says whether a pack id is downloaded,
/// `setting` is the `accelerator` setting, `active` what the service runs on.
pub fn scan(
    hw: &HardwareReport,
    measured: &[SpeedResult],
    pack_installed: impl Fn(&str) -> bool,
    setting: Option<Kind>,
    active: Option<Kind>,
) -> Vec<AccelOption> {
    let mut out = vec![];
    let mut add = |kind: Option<Kind>,
                   device: String,
                   driver_ok: bool,
                   driver_hint: &str,
                   base: Advice,
                   why: &str| {
        let pack = kind.and_then(|k| pack_for(k, hw));
        let pack_ok = pack.is_some_and(|p| pack_installed(p.id));
        let results: Vec<(String, Option<f32>, Option<f32>)> = kind
            .map(|k| {
                let label = result_label(k);
                measured
                    .iter()
                    .filter(|r| r.provider == label)
                    .map(|r| {
                        let cpu = measured
                            .iter()
                            .find(|c| c.model_id == r.model_id && c.provider == "cpu")
                            .map(|c| c.rtf);
                        (r.model_id.clone(), cpu, r.failed.is_none().then_some(r.rtf))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Measured results decide; otherwise the hardware-based guess.
        let (advice, message) = if kind.is_none() {
            (Advice::Unsupported, why.to_string())
        } else if !driver_ok {
            (Advice::NeedsDriver, driver_hint.to_string())
        } else if !results.is_empty() && results.iter().all(|r| r.2.is_none()) {
            (
                Advice::NotWorthIt,
                "Couldn't run your speech models here; correction stays on the processor"
                    .to_string(),
            )
        } else if let Some(speedup) = best_speedup(&results) {
            if speedup >= 1.15 {
                (
                    Advice::Faster,
                    format!("Correction runs {speedup:.1}× faster than on the processor"),
                )
            } else {
                (
                    Advice::NotWorthIt,
                    "Measured no faster than the processor on this PC".to_string(),
                )
            }
        } else {
            (base, why.to_string())
        };
        out.push(AccelOption {
            kind,
            device,
            advice,
            message,
            driver_ok,
            pack: pack.map(|p| (p.id.to_string(), p.name.to_string(), p.download_mb)),
            pack_installed: pack_ok,
            enabled: kind.is_some() && setting == kind,
            active: kind.is_some() && active == kind,
            results,
        });
    };

    for g in &hw.gpus {
        match g.vendor {
            GpuVendor::Nvidia => {
                let ready = g.compute_runtime && hw.cuda_major.is_some() && hw.cudnn;
                let hint = if !g.compute_runtime {
                    "Install the NVIDIA driver (Software & Updates → Additional Drivers)"
                } else {
                    "Install the CUDA runtime and cuDNN 9: sudo apt install nvidia-cuda-toolkit nvidia-cudnn"
                };
                add(
                    Some(Kind::NvidiaGpu),
                    g.name.clone(),
                    ready,
                    hint,
                    Advice::Recommended,
                    "Dedicated GPU: large speed-up expected",
                );
            }
            GpuVendor::Intel => {
                let hint = "Install the Intel compute driver: sudo apt install intel-opencl-icd libze-intel-gpu1";
                let (base, why) = if g.integrated {
                    (
                        Advice::WorthTesting,
                        "Built-in graphics: frees the processor; speed varies, so test it",
                    )
                } else {
                    (
                        Advice::Recommended,
                        "Intel Arc GPU: large speed-up expected",
                    )
                };
                add(
                    Some(Kind::IntelGpu),
                    g.name.clone(),
                    g.compute_runtime,
                    hint,
                    base,
                    why,
                );
            }
            GpuVendor::Amd => add(
                None,
                g.name.clone(),
                false,
                "",
                Advice::Unsupported,
                "AMD GPUs aren't supported by the speech engine yet",
            ),
            GpuVendor::Other => {}
        }
    }
    for n in &hw.npus {
        if n.driver == "intel_vpu" {
            add(
                Some(Kind::IntelNpu),
                n.device.clone(),
                hw.npu_runtime,
                "Install the Intel NPU driver: https://github.com/intel/linux-npu-driver",
                Advice::WorthTesting,
                "NPU: very power-efficient; speed varies, so test it",
            );
        } else {
            add(
                None,
                n.device.clone(),
                false,
                "",
                Advice::Unsupported,
                "This NPU isn't supported by the speech engine yet",
            );
        }
    }
    out
}

fn best_speedup(results: &[(String, Option<f32>, Option<f32>)]) -> Option<f32> {
    results
        .iter()
        .filter_map(|(_, cpu, acc)| Some(cpu.as_ref()? / acc.as_ref()?.max(1e-6)))
        .max_by(f32::total_cmp)
}

/// The accelerator to suggest on the Models page, if any: measured faster, or
/// a dedicated GPU that's ready to set up.
pub fn suggestion(options: &[AccelOption]) -> Option<&AccelOption> {
    options
        .iter()
        .filter(|o| !o.enabled && o.kind.is_some())
        .find(|o| o.advice == Advice::Faster)
        .or_else(|| {
            options
                .iter()
                .filter(|o| !o.enabled)
                .find(|o| o.advice == Advice::Recommended)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{CpuInfo, GpuInfo, MemoryInfo};

    fn hw(gpus: Vec<GpuInfo>) -> HardwareReport {
        HardwareReport {
            cpu: CpuInfo {
                model: "x".into(),
                physical_cores: 4,
                logical_cpus: 4,
                max_mhz: None,
                avx2: true,
                avx512: false,
                vnni: false,
                amx: false,
            },
            memory: MemoryInfo {
                total_mb: 8000,
                available_mb: 4000,
            },
            gpus,
            npus: vec![],
            on_battery: false,
            fingerprint: "f".into(),
            cuda_major: Some(12),
            cudnn: true,
            npu_runtime: false,
        }
    }

    fn gpu(vendor: GpuVendor, integrated: bool, runtime: bool) -> GpuInfo {
        GpuInfo {
            vendor,
            name: "g".into(),
            pci_id: "0:0".into(),
            driver: "d".into(),
            integrated,
            vram_mb: None,
            render_node: None,
            compute_runtime: runtime,
        }
    }

    fn speed(id: &str, provider: &str, rtf: f32) -> SpeedResult {
        SpeedResult {
            model_id: id.into(),
            provider: provider.into(),
            threads: 2,
            rtf,
            peak_rss_mb: 0,
            load_ms: 0,
            chunk_p95_ms: None,
            failed: None,
        }
    }

    #[test]
    fn advice_follows_hardware_then_measurements() {
        let h = hw(vec![
            gpu(GpuVendor::Nvidia, false, true),
            gpu(GpuVendor::Intel, true, false),
            gpu(GpuVendor::Amd, false, false),
        ]);
        let o = scan(&h, &[], |_| false, None, None);
        assert_eq!(o[0].advice, Advice::Recommended);
        assert_eq!(o[1].advice, Advice::NeedsDriver);
        assert_eq!(o[2].advice, Advice::Unsupported);
        assert_eq!(suggestion(&o).unwrap().kind, Some(Kind::NvidiaGpu));

        let h = hw(vec![gpu(GpuVendor::Intel, true, true)]);
        assert_eq!(
            scan(&h, &[], |_| true, None, None)[0].advice,
            Advice::WorthTesting
        );
        let slower = [speed("p", "cpu", 0.25), speed("p", "accel:intel-gpu", 0.30)];
        assert_eq!(
            scan(&h, &slower, |_| true, None, None)[0].advice,
            Advice::NotWorthIt
        );
        let mut broken = speed("p", "accel:intel-gpu", 0.0);
        broken.failed = Some("crash".into());
        let o = scan(&h, &[speed("p", "cpu", 0.25), broken], |_| true, None, None);
        assert_eq!(o[0].advice, Advice::NotWorthIt);
        let faster = [speed("p", "cpu", 0.25), speed("p", "accel:intel-gpu", 0.10)];
        let o = scan(&h, &faster, |_| true, None, None);
        assert_eq!(o[0].advice, Advice::Faster);
        assert_eq!(suggestion(&o).unwrap().kind, Some(Kind::IntelGpu));
    }
}
