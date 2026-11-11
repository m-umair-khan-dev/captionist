//! Caption history entries and export formats (.txt, .srt, .vtt).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// Unix time (seconds) when the sentence started.
    pub at: f64,
    /// Sentence length in seconds.
    pub duration: f64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Srt,
    Vtt,
}

impl Format {
    /// From a file name's extension; plain text otherwise.
    pub fn from_path(path: &str) -> Self {
        let p = path.to_ascii_lowercase();
        if p.ends_with(".srt") {
            Format::Srt
        } else if p.ends_with(".vtt") {
            Format::Vtt
        } else {
            Format::Text
        }
    }
}

/// Local wall-clock "HH:MM:SS" for a Unix time (uses the TZ offset given).
pub fn clock(at: f64, utc_offset_s: i64) -> String {
    let t = (at as i64 + utc_offset_s).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

fn stamp(secs: f64, sep: char) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{sep}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Renders entries. Subtitle times are relative to the first entry.
pub fn export(entries: &[Entry], format: Format, utc_offset_s: i64) -> String {
    let t0 = entries.first().map(|e| e.at).unwrap_or(0.0);
    let mut out = String::new();
    match format {
        Format::Text => {
            for e in entries {
                out += &format!("[{}] {}\n", clock(e.at, utc_offset_s), e.text);
            }
        }
        Format::Srt | Format::Vtt => {
            if format == Format::Vtt {
                out += "WEBVTT\n\n";
            }
            let sep = if format == Format::Srt { ',' } else { '.' };
            for (i, e) in entries.iter().enumerate() {
                let start = e.at - t0;
                // Keep each caption on screen at least 1.5 s, but not past the next one.
                let mut end = start + e.duration.max(1.5);
                if let Some(next) = entries.get(i + 1) {
                    end = end.min(next.at - t0).max(start + 0.5);
                }
                if format == Format::Srt {
                    out += &format!("{}\n", i + 1);
                }
                out += &format!(
                    "{} --> {}\n{}\n\n",
                    stamp(start, sep),
                    stamp(end, sep),
                    e.text
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_formats() {
        let e = vec![
            Entry {
                at: 3600.0,
                duration: 2.0,
                text: "Hello there.".into(),
            },
            Entry {
                at: 3605.5,
                duration: 1.0,
                text: "How are you?".into(),
            },
        ];
        assert_eq!(
            export(&e, Format::Text, 0),
            "[01:00:00] Hello there.\n[01:00:05] How are you?\n"
        );
        let srt = export(&e, Format::Srt, 0);
        assert!(srt.starts_with(
            "1\n00:00:00,000 --> 00:00:02,000\nHello there.\n\n2\n00:00:05,500 --> 00:00:07,000\n"
        ));
        assert!(
            export(&e, Format::Vtt, 0).starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n")
        );
        assert_eq!(Format::from_path("talk.SRT"), Format::Srt);
    }
}
