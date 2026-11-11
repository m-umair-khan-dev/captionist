//! Hardware probe: CPU, memory, GPUs, NPUs and power state.
//!
//! Reads only `/proc` and `/sys` (plus `lspci` / `nvidia-smi` when present for
//! nicer names), so it works without root and without extra libraries.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareReport {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub gpus: Vec<GpuInfo>,
    pub npus: Vec<NpuInfo>,
    pub on_battery: bool,
    /// Major version of the CUDA runtime libraries found (12, 13), if any.
    #[serde(default)]
    pub cuda_major: Option<u32>,
    /// cuDNN 9 found (needed by ONNX Runtime's CUDA provider).
    #[serde(default)]
    pub cudnn: bool,
    /// Intel NPU user-space driver found.
    #[serde(default)]
    pub npu_runtime: bool,
    /// Changes when the CPU, GPUs or RAM size change; used to invalidate cached speed tests.
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub model: String,
    pub physical_cores: usize,
    pub logical_cpus: usize,
    pub max_mhz: Option<u32>,
    pub avx2: bool,
    pub avx512: bool,
    /// AVX-VNNI or AVX512-VNNI: speeds up int8 models.
    pub vnni: bool,
    pub amx: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_mb: u64,
    pub available_mb: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Intel,
    Amd,
    Nvidia,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub name: String,
    pub pci_id: String,
    pub driver: String,
    pub integrated: bool,
    pub vram_mb: Option<u64>,
    pub render_node: Option<String>,
    /// Whether the user-space compute stack needed for ML is installed
    /// (Intel: Level Zero / OpenCL, NVIDIA: libcuda, AMD: ROCm).
    pub compute_runtime: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpuInfo {
    pub device: String,
    pub driver: String,
}

impl HardwareReport {
    pub fn probe() -> Self {
        let cpu = probe_cpu();
        let memory = probe_memory();
        let gpus = probe_gpus();
        let npus = probe_npus();
        let on_battery = probe_on_battery();
        let fingerprint = fingerprint(&cpu, &memory, &gpus, &npus);
        let cuda_major = if lib_exists(&["libcublasLt.so.13", "libcudart.so.13"]) {
            Some(13)
        } else if lib_exists(&["libcublasLt.so.12", "libcudart.so.12"]) {
            Some(12)
        } else {
            None
        };
        Self {
            cpu,
            memory,
            gpus,
            npus,
            on_battery,
            cuda_major,
            cudnn: lib_exists(&["libcudnn.so.9"]),
            npu_runtime: lib_exists(&["libze_intel_vpu.so.1", "libnpu_driver_compiler.so"]),
            fingerprint,
        }
    }

    /// Threads to give each recognition pass. Live and refine passes run at the
    /// same time, so each gets about half the cores, and at least one core is
    /// left for the desktop on bigger machines.
    pub fn threads_per_pass(&self) -> i32 {
        let cores = self.cpu.physical_cores.max(1);
        ((cores / 2).clamp(1, 4)) as i32
    }

    pub fn human_summary(&self) -> String {
        let mut s = String::new();
        let c = &self.cpu;
        let mut isa = vec![];
        if c.avx2 {
            isa.push("AVX2");
        }
        if c.avx512 {
            isa.push("AVX-512");
        }
        if c.vnni {
            isa.push("VNNI");
        }
        if c.amx {
            isa.push("AMX");
        }
        s += &format!(
            "CPU     {} — {} cores / {} threads{}{}\n",
            c.model,
            c.physical_cores,
            c.logical_cpus,
            c.max_mhz
                .map(|m| format!(", up to {:.1} GHz", m as f32 / 1000.0))
                .unwrap_or_default(),
            if isa.is_empty() {
                String::new()
            } else {
                format!(" [{}]", isa.join(", "))
            }
        );
        s += &format!(
            "Memory  {:.1} GB total, {:.1} GB available\n",
            self.memory.total_mb as f32 / 1024.0,
            self.memory.available_mb as f32 / 1024.0
        );
        if self.gpus.is_empty() {
            s += "GPU     none detected\n";
        }
        for g in &self.gpus {
            s += &format!(
                "GPU     {} ({}{}, driver {}, compute runtime: {})\n",
                g.name,
                if g.integrated {
                    "integrated"
                } else {
                    "dedicated"
                },
                g.vram_mb
                    .map(|v| format!(", {v} MB VRAM"))
                    .unwrap_or_default(),
                g.driver,
                if g.compute_runtime {
                    "installed"
                } else {
                    "missing"
                }
            );
        }
        if self.npus.is_empty() {
            s += "NPU     none detected\n";
        }
        for n in &self.npus {
            s += &format!("NPU     {} (driver {})\n", n.device, n.driver);
        }
        s += &format!(
            "Power   {}\n",
            if self.on_battery {
                "on battery"
            } else {
                "mains"
            }
        );
        s
    }
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn probe_cpu() -> CpuInfo {
    let text = read("/proc/cpuinfo").unwrap_or_default();
    let mut model = String::from("Unknown CPU");
    let mut flags: HashSet<String> = HashSet::new();
    let mut logical = 0usize;
    let mut cores: HashSet<(String, String)> = HashSet::new();
    let (mut phys_id, mut core_id) = (String::new(), String::new());

    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            // Blank line ends one processor block.
            if !core_id.is_empty() {
                cores.insert((phys_id.clone(), core_id.clone()));
            }
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "processor" => logical += 1,
            "model name" => model = v.to_string(),
            "physical id" => phys_id = v.to_string(),
            "core id" => core_id = v.to_string(),
            "flags" if flags.is_empty() => {
                flags = v.split_whitespace().map(str::to_string).collect();
            }
            _ => {}
        }
    }
    if !core_id.is_empty() {
        cores.insert((phys_id, core_id));
    }
    let logical = logical.max(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    );
    let physical = if cores.is_empty() {
        logical
    } else {
        cores.len()
    };
    let max_mhz = read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .and_then(|s| s.parse::<u32>().ok())
        .map(|khz| khz / 1000);
    let has = |f: &str| flags.contains(f);

    CpuInfo {
        model,
        physical_cores: physical,
        logical_cpus: logical,
        max_mhz,
        avx2: has("avx2"),
        avx512: has("avx512f"),
        vnni: has("avx_vnni") || has("avx512_vnni"),
        amx: has("amx_tile"),
    }
}

fn probe_memory() -> MemoryInfo {
    let text = read("/proc/meminfo").unwrap_or_default();
    let field = |name: &str| {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|kb| kb / 1024)
            .unwrap_or(0)
    };
    MemoryInfo {
        total_mb: field("MemTotal:"),
        available_mb: field("MemAvailable:"),
    }
}

fn lib_exists(names: &[&str]) -> bool {
    const DIRS: &[&str] = &[
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
        "/usr/lib/x86_64-linux-gnu/intel-opencl",
        "/opt/rocm/lib",
        "/usr/local/cuda/lib64",
        "/usr/local/cuda/targets/x86_64-linux/lib",
        "/usr/lib/x86_64-linux-gnu/nvidia/current",
    ];
    DIRS.iter()
        .any(|d| names.iter().any(|n| Path::new(d).join(n).exists()))
}

fn lspci_name(slot: &str) -> Option<String> {
    // `lspci -mm -s <slot>` prints: Slot "Class" "Vendor" "Device" ...
    let out = Command::new("lspci")
        .args(["-mm", "-s", slot])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = line.split('"').collect();
    // parts: [slot, class, ' ', vendor, ' ', device, ...]
    let vendor = parts.get(3)?.trim();
    let device = parts.get(5)?.trim();
    let vendor = vendor.replace(" Corporation", "").replace(", Inc.", "");
    Some(format!("{vendor} {device}"))
}

fn nvidia_vram_mb() -> Option<u64> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

fn probe_gpus() -> Vec<GpuInfo> {
    let mut gpus = vec![];
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return gpus;
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("card") && !n.contains('-'))
        .collect();
    cards.sort();

    for card in cards {
        let dev = Path::new("/sys/class/drm").join(&card).join("device");
        let vendor_id = read(dev.join("vendor")).unwrap_or_default();
        let device_id = read(dev.join("device")).unwrap_or_default();
        let driver = fs::read_link(dev.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".into());
        let slot = fs::canonicalize(&dev)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        let vendor = match vendor_id.as_str() {
            "0x8086" => GpuVendor::Intel,
            "0x1002" => GpuVendor::Amd,
            "0x10de" => GpuVendor::Nvidia,
            _ => GpuVendor::Other,
        };
        if vendor == GpuVendor::Other && driver == "unknown" {
            continue; // virtual / simple framebuffer
        }
        let pci_id = format!(
            "{}:{}",
            vendor_id.trim_start_matches("0x"),
            device_id.trim_start_matches("0x")
        );
        let name = lspci_name(&slot).unwrap_or_else(|| format!("{vendor:?} GPU [{pci_id}]"));
        let render_node = fs::read_dir(dev.join("drm")).ok().and_then(|it| {
            it.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .find(|n| n.starts_with("renderD"))
                .map(|n| format!("/dev/dri/{n}"))
        });
        let vram_mb = match vendor {
            GpuVendor::Amd => read(dev.join("mem_info_vram_total"))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|b| b / 1_048_576),
            GpuVendor::Nvidia => nvidia_vram_mb(),
            _ => None,
        };
        let integrated = match vendor {
            GpuVendor::Intel => !name.contains("Arc"),
            GpuVendor::Amd => vram_mb.is_none_or(|v| v < 2048),
            GpuVendor::Nvidia => false,
            GpuVendor::Other => true,
        };
        let compute_runtime = match vendor {
            GpuVendor::Intel => lib_exists(&["libze_intel_gpu.so.1", "libigdrcl.so"]),
            GpuVendor::Nvidia => lib_exists(&["libcuda.so.1"]),
            GpuVendor::Amd => lib_exists(&["libamdhip64.so", "libMIOpen.so.1"]),
            GpuVendor::Other => false,
        };
        gpus.push(GpuInfo {
            vendor,
            name,
            pci_id,
            driver,
            integrated,
            vram_mb,
            render_node,
            compute_runtime,
        });
    }
    gpus
}

fn probe_npus() -> Vec<NpuInfo> {
    let mut npus = vec![];
    let Ok(entries) = fs::read_dir("/sys/class/accel") else {
        return npus;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let driver = fs::read_link(e.path().join("device/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".into());
        let label = match driver.as_str() {
            "intel_vpu" => "Intel NPU".to_string(),
            "amdxdna" => "AMD XDNA NPU".to_string(),
            _ => name.clone(),
        };
        npus.push(NpuInfo {
            device: format!("/dev/accel/{name} ({label})"),
            driver,
        });
    }
    npus
}

fn probe_on_battery() -> bool {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return false;
    };
    let mut has_mains = false;
    let mut mains_online = false;
    for e in entries.flatten() {
        let p = e.path();
        if read(p.join("type")).as_deref() == Some("Mains") {
            has_mains = true;
            mains_online |= read(p.join("online")).as_deref() == Some("1");
        }
    }
    has_mains && !mains_online
}

fn fingerprint(cpu: &CpuInfo, mem: &MemoryInfo, gpus: &[GpuInfo], npus: &[NpuInfo]) -> String {
    // FNV-1a, stable across runs and Rust versions.
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |s: &str| {
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    feed(&cpu.model);
    feed(&cpu.logical_cpus.to_string());
    feed(&(mem.total_mb / 1024).to_string());
    for g in gpus {
        feed(&g.pci_id);
        feed(&g.compute_runtime.to_string());
    }
    for n in npus {
        feed(&n.driver);
    }
    format!("{h:016x}")
}
