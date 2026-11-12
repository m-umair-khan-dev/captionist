//! Optional hardware acceleration: downloading runtime packs and running the
//! service on them. See `captionist_core::accel` for detection and advice.
//!
//! A pack is a vendor build of ONNX Runtime 1.24.1 (the version Captionist's
//! sherpa-onnx is built against) with an accelerator provider: Intel OpenVINO
//! (GPU/NPU) or NVIDIA CUDA. It's downloaded on request, checksum-verified and
//! unpacked to `~/.local/share/captionist/accel/<pack>`. The caption service then
//! restarts itself with that folder first on the library path.

use anyhow::{Context, Result, bail};
use captionist_core::accel::{Archive, Kind, Pack};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Set in the service's environment to the accelerator it was started for.
pub const ENV_ACTIVE: &str = "CAPTIONIST_ACCEL";

pub fn packs_root() -> PathBuf {
    captionist_core::catalog::default_models_root()
        .parent()
        .map(|p| p.join("accel"))
        .unwrap_or_else(|| PathBuf::from("accel"))
}

/// Library search path for running on `kind` with `pack`: the pack first, then
/// (for NVIDIA) CUDA toolkit folders that the system's library loader may not
/// know about, so the CUDA provider finds cuBLAS, cuFFT and friends.
pub fn library_path(kind: Kind, pack: &Pack) -> std::ffi::OsString {
    let mut dirs = vec![pack_dir(pack.id)];
    if kind == Kind::NvidiaGpu {
        dirs.extend(captionist_core::hardware::cuda_lib_dirs());
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

pub fn pack_dir(pack_id: &str) -> PathBuf {
    packs_root().join(pack_id)
}

pub fn pack_installed(pack_id: &str) -> bool {
    pack_dir(pack_id).join("libonnxruntime.so.1").exists()
}

/// The accelerator this process was started with, if any.
pub fn active() -> Option<Kind> {
    std::env::var(ENV_ACTIVE).ok().and_then(|v| Kind::parse(&v))
}

/// sherpa-onnx provider string for running on `kind`.
pub fn provider(kind: Kind) -> Result<String> {
    match kind {
        Kind::NvidiaGpu => Ok("cuda".into()),
        Kind::IntelGpu | Kind::IntelNpu => {
            let device = if kind == Kind::IntelGpu { "GPU" } else { "NPU" };
            // No model cache: OpenVINO stores one compiled copy per input
            // length, which grew to gigabytes in testing. FP32 because FP16
            // produced wrong text on Intel graphics.
            fs::create_dir_all(crate::paths::state_dir())?;
            let cfg =
                crate::paths::state_dir().join(format!("openvino-{}.conf", device.to_lowercase()));
            let precision = if kind == Kind::IntelGpu {
                "precision=FP32\n"
            } else {
                ""
            };
            fs::write(&cfg, format!("device_type={device}\n{precision}"))?;
            Ok(format!("openvino:{}", cfg.display()))
        }
    }
}

fn download_verified(pack: &Pack, dest: &Path, mut progress: impl FnMut(f64)) -> Result<()> {
    let resp = ureq::get(pack.url)
        .call()
        .with_context(|| format!("downloading {}", pack.name))?;
    let total = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(pack.download_mb as u64 * 1_048_576);
    let mut reader = resp.into_body().into_reader();
    let mut file = fs::File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let (mut read, mut last) = (0u64, -1i64);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        read += n as u64;
        let pct = (read * 100 / total.max(1)) as i64;
        if pct != last {
            last = pct;
            progress((read as f64 / total as f64).min(1.0));
        }
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != pack.sha256 {
        bail!("{} failed its checksum check", pack.name);
    }
    Ok(())
}

/// Keeps only native libraries (`*.so*`) directly inside `lib_dir`, either at
/// the archive's root or under one top-level folder of any name.
fn wanted(path: &str, lib_dir: &str) -> Option<String> {
    let path = Path::new(path.trim_start_matches("./"));
    let base = path.file_name()?.to_str()?;
    let parent = path.parent()?;
    let depth = Path::new(lib_dir).components().count();
    let in_lib_dir = parent.ends_with(lib_dir)
        && (parent.components().count() == depth || parent.components().count() == depth + 1);
    if !in_lib_dir || !base.contains(".so") || base.starts_with("onnxruntime_pybind") {
        return None;
    }
    Some(base.to_string())
}

/// Downloads, verifies and unpacks `pack`. Reports progress 0–1.
pub fn install_pack(pack: &Pack, progress: impl FnMut(f64)) -> Result<()> {
    let dir = pack_dir(pack.id);
    let tmp = packs_root().join(format!(".download-{}", pack.id));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<()> {
        let archive = tmp.join("pack");
        download_verified(pack, &archive, progress)?;
        let out = tmp.join("libs");
        fs::create_dir_all(&out)?;
        match pack.archive {
            Archive::Wheel => {
                let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
                for i in 0..zip.len() {
                    let mut f = zip.by_index(i)?;
                    let name = f.name()?.to_string();
                    let Some(base) = wanted(&name, pack.lib_dir) else {
                        continue;
                    };
                    let mut data = vec![];
                    f.read_to_end(&mut data)?;
                    fs::write(out.join(base), data)?;
                }
            }
            Archive::TarGz => {
                let mut tar =
                    tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&archive)?));
                for entry in tar.entries()? {
                    let mut e = entry?;
                    let path = e.path()?.to_string_lossy().to_string();
                    let Some(base) = wanted(&path, pack.lib_dir) else {
                        continue;
                    };
                    if e.header().entry_type().is_symlink() {
                        if let Some(target) = e.link_name()? {
                            let _ = std::os::unix::fs::symlink(target, out.join(&base));
                        }
                    } else {
                        e.unpack(out.join(&base))?;
                    }
                }
            }
        }
        let ort = fs::read_dir(&out)?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|n| n.starts_with("libonnxruntime.so.1.") && !n.contains("providers"))
            .with_context(|| {
                format!(
                    "{}: no ONNX Runtime library found in the download",
                    pack.name
                )
            })?;
        for link in ["libonnxruntime.so.1", "libonnxruntime.so"] {
            if !out.join(link).exists() {
                std::os::unix::fs::symlink(&ort, out.join(link))?;
            }
        }
        let _ = fs::remove_dir_all(&dir);
        fs::rename(&out, &dir)?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&tmp);
    result?;
    if !pack_installed(pack.id) {
        bail!("{} is incomplete", pack.name);
    }
    Ok(())
}

pub fn remove_pack(pack_id: &str) -> Result<()> {
    let d = pack_dir(pack_id);
    if d.exists() {
        fs::remove_dir_all(&d)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::wanted;

    #[test]
    fn finds_libraries_whatever_the_top_folder_is_called() {
        // The CUDA 13 archive reuses the CUDA 12 folder name.
        for top in [
            "onnxruntime-linux-x64-gpu-1.24.1",
            "onnxruntime-linux-x64-gpu_cuda13-1.24.1",
        ] {
            assert_eq!(
                wanted(&format!("{top}/lib/libonnxruntime.so.1.24.1"), "lib").as_deref(),
                Some("libonnxruntime.so.1.24.1")
            );
            assert_eq!(
                wanted(&format!("{top}/lib/cmake/onnxruntime/x.so"), "lib"),
                None
            );
            assert_eq!(wanted(&format!("{top}/include/x.so"), "lib"), None);
        }
        assert_eq!(
            wanted("./lib/libonnxruntime_providers_cuda.so", "lib").as_deref(),
            Some("libonnxruntime_providers_cuda.so")
        );
        // Wheels keep the libraries in onnxruntime/capi/.
        assert_eq!(
            wanted(
                "onnxruntime/capi/libonnxruntime.so.1.24.1",
                "onnxruntime/capi"
            )
            .as_deref(),
            Some("libonnxruntime.so.1.24.1")
        );
        assert_eq!(
            wanted(
                "onnxruntime/capi/onnxruntime_pybind11_state.so",
                "onnxruntime/capi"
            ),
            None
        );
        assert_eq!(
            wanted("onnxruntime/capi/lib/x.so", "onnxruntime/capi"),
            None
        );
    }
}
