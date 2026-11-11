//! Where Captionist keeps its files.

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

/// Downloaded models: `~/.local/share/captionist/models`.
pub fn models_root() -> PathBuf {
    captionist_core::catalog::default_models_root()
}

/// Speed-test results etc.: `~/.local/state/captionist`.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("captionist")
}

/// Read-only data shipped with Captionist (calibration clip, VAD model).
/// Looks in the user's data dir first (local install), then system dirs (.deb).
pub fn data_file(name: &str) -> Option<PathBuf> {
    let mut dirs = vec![xdg("XDG_DATA_HOME", ".local/share")];
    let sys =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs.into_iter()
        .map(|d| d.join("captionist").join(name))
        .find(|p| p.exists())
}
