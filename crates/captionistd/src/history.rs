//! Caption history: kept in memory for the session, and optionally on disk as
//! one JSON-lines file per day in `~/.local/state/captionist/history/`.

use captionist_core::history::Entry;
use captionist_engine::paths;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Entries kept in memory (oldest dropped first).
const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Session,
    Keep,
}

impl Mode {
    pub fn parse(s: &str) -> Self {
        match s {
            "off" => Mode::Off,
            "keep" => Mode::Keep,
            _ => Mode::Session,
        }
    }
}

pub struct History {
    pub mode: Mode,
    pub days: i32,
    pub entries: Vec<Entry>,
}

fn dir() -> PathBuf {
    paths::state_dir().join("history")
}

fn day_file(at: f64) -> PathBuf {
    let day = glib::DateTime::from_unix_local(at as i64)
        .ok()
        .and_then(|d| d.format("%Y-%m-%d").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".into());
    dir().join(format!("{day}.jsonl"))
}

impl History {
    pub fn new() -> Self {
        History {
            mode: Mode::Session,
            days: 30,
            entries: vec![],
        }
    }

    /// Applies the settings; loads saved history when keeping it on disk.
    pub fn configure(&mut self, mode: Mode, days: i32) {
        let changed = mode != self.mode || days != self.days;
        self.mode = mode;
        self.days = days.max(1);
        match mode {
            Mode::Off => self.entries.clear(),
            Mode::Keep if changed => {
                self.prune_files();
                let mut saved = self.load_files();
                // Keep this session's entries that aren't on disk yet.
                let last_saved = saved.last().map(|e| e.at).unwrap_or(0.0);
                saved.extend(self.entries.drain(..).filter(|e| e.at > last_saved));
                self.entries = saved;
                self.save_all_unsaved(last_saved);
            }
            _ => {}
        }
    }

    pub fn add(&mut self, e: Entry) {
        if self.mode == Mode::Off || e.text.trim().is_empty() {
            return;
        }
        if self.mode == Mode::Keep {
            let _ = fs::create_dir_all(dir());
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(day_file(e.at))
            {
                let _ = writeln!(f, "{}", serde_json::to_string(&e).unwrap_or_default());
            }
        }
        self.entries.push(e);
        if self.entries.len() > MAX_ENTRIES {
            let extra = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..extra);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        let _ = fs::remove_dir_all(dir());
    }

    fn save_all_unsaved(&self, after: f64) {
        let _ = fs::create_dir_all(dir());
        for e in self.entries.iter().filter(|e| e.at > after) {
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(day_file(e.at))
            {
                let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default());
            }
        }
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(dir())
            .map(|it| {
                it.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    fn load_files(&self) -> Vec<Entry> {
        let mut out = vec![];
        for f in self.files() {
            if let Ok(text) = fs::read_to_string(&f) {
                out.extend(
                    text.lines()
                        .filter_map(|l| serde_json::from_str::<Entry>(l).ok()),
                );
            }
        }
        out.sort_by(|a, b| a.at.total_cmp(&b.at));
        let keep = out.len().saturating_sub(MAX_ENTRIES);
        out.drain(..keep);
        out
    }

    /// Deletes day files older than `days`.
    fn prune_files(&self) {
        let cutoff = glib::DateTime::now_local()
            .ok()
            .and_then(|d| d.add_days(-self.days).ok())
            .and_then(|d| d.format("%Y-%m-%d").ok())
            .map(|s| format!("{s}.jsonl"));
        let Some(cutoff) = cutoff else { return };
        for f in self.files() {
            if f.file_name()
                .is_some_and(|n| n.to_string_lossy().as_ref() < cutoff.as_str())
            {
                let _ = fs::remove_file(f);
            }
        }
    }
}
