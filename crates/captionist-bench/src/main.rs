//! captionist-bench: hardware probe, model speed test, accuracy evaluation and
//! model recommendation.
//!
//!   captionist-bench probe                 show what this PC has
//!   captionist-bench speed                 time the small calibration model, estimate the rest, recommend
//!   captionist-bench speed --all           time every downloaded model instead of estimating
//!   captionist-bench list -r FILE          sorted model list from saved results
//!   captionist-bench eval                  full accuracy + speed evaluation (developers)
//!   captionist-bench recommend -r FILE     recommendation from saved results
//!   captionist-bench assess -m ID -r FILE  verdict for forcing one model

mod wer;

use captionist_engine::asr;

use anyhow::{Context, Result, bail};
use captionist_core::catalog::{Catalog, Role};
use captionist_core::hardware::HardwareReport;
use captionist_core::listing::{self, Fit, SortBy, SpeedSource};
use captionist_core::recommend::{self, Limits, Recommendation, SpeedResult};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const RESULT_PREFIX: &str = "CAPTIONIST_RESULT ";

#[derive(Parser)]
#[command(
    version,
    about = "Captionist hardware check, model speed test and accuracy evaluation"
)]
struct Cli {
    /// Model catalogue (default: built-in copy of data/models.toml)
    #[arg(long, global = true)]
    catalog: Option<PathBuf>,
    /// Folder holding the extracted models
    #[arg(long, global = true, default_value = ".bench/models")]
    models_dir: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct RunArgs {
    /// Model ids to test (default: every installed model)
    #[arg(long, value_delimiter = ',')]
    models: Vec<String>,
    /// Test sets under --data (folders containing refs.tsv + wav/)
    #[arg(long, value_delimiter = ',')]
    sets: Vec<String>,
    #[arg(long, default_value = ".bench/data")]
    data: PathBuf,
    /// Utterances per set, spread evenly over speakers
    #[arg(long)]
    max_utts: Option<usize>,
    /// Threads per model (default: half the CPU cores, as in real use)
    #[arg(long)]
    threads: Option<i32>,
    /// ONNX Runtime provider: cpu, cuda, openvino ...
    #[arg(long, default_value = "cpu")]
    provider: String,
    /// Write all results as JSON here
    #[arg(long, short)]
    out: Option<PathBuf>,
    /// speed: time every downloaded model instead of only the calibration model
    #[arg(long)]
    all: bool,
    /// Model list order: recommended, accuracy, speed, download, memory
    #[arg(long, default_value = "recommended")]
    sort: SortBy,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show detected CPU, memory, GPUs and NPUs
    Probe {
        #[arg(long)]
        json: bool,
    },
    /// Quick speed test of installed models, then a recommendation
    Speed(RunArgs),
    /// Full accuracy (WER) + speed evaluation
    Eval(RunArgs),
    /// Sorted model list (with estimates) from a saved results file
    List {
        #[arg(long, short)]
        results: PathBuf,
        #[arg(long, default_value = "recommended")]
        sort: SortBy,
    },
    /// Recommendation from a saved results file
    Recommend {
        #[arg(long, short)]
        results: PathBuf,
    },
    /// Judge forcing a particular model
    Assess {
        #[arg(long, short)]
        model: String,
        #[arg(long, short)]
        results: PathBuf,
    },
    /// Internal: test one model in a fresh process (clean memory numbers)
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        model: String,
        #[arg(long, value_delimiter = ',')]
        sets: Vec<String>,
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        max_utts: Option<usize>,
        #[arg(long)]
        threads: i32,
        #[arg(long)]
        provider: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SetScore {
    set: String,
    utts: usize,
    audio_s: f32,
    ref_words: usize,
    errors: usize,
    wer: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerReport {
    speed: SpeedResult,
    scores: Vec<SetScore>,
    /// A few (reference, hypothesis) pairs for eyeballing.
    samples: Vec<(String, String)>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RunFile {
    hardware: HardwareReport,
    reports: Vec<WorkerReport>,
    failures: Vec<(String, String)>,
    recommendation: Recommendation,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let catalog = match &cli.catalog {
        Some(p) => Catalog::load(p)?,
        None => Catalog::embedded(),
    };

    match cli.cmd {
        Cmd::Probe { json } => {
            let hw = HardwareReport::probe();
            if json {
                println!("{}", serde_json::to_string_pretty(&hw)?);
            } else {
                print!("{}", hw.human_summary());
                println!("Threads per pass: {}", hw.threads_per_pass());
            }
        }
        Cmd::Speed(mut a) => {
            if a.sets.is_empty() {
                a.sets = vec!["test-clean".into()];
            }
            a.max_utts.get_or_insert(10);
            if !a.all && a.models.is_empty() {
                let cal = catalog
                    .calibration_model()
                    .context("catalogue has no calibration model")?;
                if !cal.is_installed(&cli.models_dir) {
                    bail!(
                        "calibration model '{}' is not downloaded ({} MB): {}",
                        cal.id,
                        cal.download_mb,
                        cal.url
                    );
                }
                a.models = vec![cal.id.clone()];
            }
            run(&catalog, cli.catalog.as_deref(), &cli.models_dir, a)?;
        }
        Cmd::Eval(mut a) => {
            if a.sets.is_empty() {
                a.sets = vec!["test-clean".into(), "test-other".into()];
            }
            a.max_utts.get_or_insert(60);
            run(&catalog, cli.catalog.as_deref(), &cli.models_dir, a)?;
        }
        Cmd::List { results, sort } => {
            let f: RunFile = serde_json::from_str(&std::fs::read_to_string(&results)?)?;
            let measured: Vec<_> = f.reports.iter().map(|r| r.speed.clone()).collect();
            let hw = HardwareReport::probe();
            let rec = recommend_with_estimates(&hw, &catalog, &measured);
            print_model_list(&catalog, &hw, &measured, &rec, &cli.models_dir, sort);
        }
        Cmd::Recommend { results } => {
            let f: RunFile = serde_json::from_str(&std::fs::read_to_string(&results)?)?;
            let measured: Vec<_> = f.reports.iter().map(|r| r.speed.clone()).collect();
            let rec = recommend_with_estimates(&HardwareReport::probe(), &catalog, &measured);
            print_recommendation(&rec);
        }
        Cmd::Assess { model, results } => {
            let f: RunFile = serde_json::from_str(&std::fs::read_to_string(&results)?)?;
            let m = catalog
                .get(&model)
                .with_context(|| format!("unknown model {model}"))?;
            let r = f
                .reports
                .iter()
                .map(|r| &r.speed)
                .find(|s| s.model_id == model);
            let v = recommend::assess(m, r, Limits::default());
            println!("{} {}", if v.ok { "OK  " } else { "WARN" }, v.message);
        }
        Cmd::Worker {
            model,
            sets,
            data,
            max_utts,
            threads,
            provider,
        } => {
            let report = worker(
                &catalog,
                &cli.models_dir,
                &model,
                &sets,
                &data,
                max_utts,
                threads,
                &provider,
            )?;
            println!("{RESULT_PREFIX}{}", serde_json::to_string(&report)?);
        }
    }
    Ok(())
}

/// Runs each model in its own process and collects the results.
fn run(
    catalog: &Catalog,
    catalog_path: Option<&Path>,
    models_dir: &Path,
    a: RunArgs,
) -> Result<()> {
    let hw = HardwareReport::probe();
    let threads = a.threads.unwrap_or_else(|| hw.threads_per_pass());
    print!("{}", hw.human_summary());
    println!(
        "Testing with {threads} thread(s) per model, provider {}\n",
        a.provider
    );

    let ids: Vec<String> = if a.models.is_empty() {
        catalog
            .models
            .iter()
            .filter(|m| m.is_installed(models_dir))
            .map(|m| m.id.clone())
            .collect()
    } else {
        a.models.clone()
    };
    if ids.is_empty() {
        bail!("no installed models found in {}", models_dir.display());
    }

    let exe = std::env::current_exe()?;
    let mut reports = vec![];
    let mut failures = vec![];
    for id in &ids {
        eprint!("  {id:<28} … ");
        let mut cmd = Command::new(&exe);
        if let Some(c) = catalog_path {
            cmd.arg("--catalog").arg(c);
        }
        cmd.arg("--models-dir")
            .arg(models_dir)
            .args(["worker", "--model", id]);
        cmd.arg("--sets").arg(a.sets.join(","));
        cmd.arg("--data").arg(&a.data);
        cmd.arg("--threads").arg(threads.to_string());
        cmd.arg("--provider").arg(&a.provider);
        if let Some(n) = a.max_utts {
            cmd.arg("--max-utts").arg(n.to_string());
        }
        let out = cmd.output()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        match stdout.lines().find_map(|l| l.strip_prefix(RESULT_PREFIX)) {
            Some(json) => {
                let r: WorkerReport = serde_json::from_str(json)?;
                eprintln!("RTF {:.3}", r.speed.rtf);
                reports.push(r);
            }
            None => {
                let err = String::from_utf8_lossy(&out.stderr);
                let last = err
                    .lines()
                    .rfind(|l| !l.trim().is_empty())
                    .unwrap_or("crashed")
                    .to_string();
                eprintln!("FAILED: {last}");
                failures.push((id.clone(), last));
            }
        }
    }

    print_table(catalog, &reports);
    let measured: Vec<_> = reports.iter().map(|r| r.speed.clone()).collect();
    let rec = recommend_with_estimates(&hw, catalog, &measured);
    print_recommendation(&rec);
    print_model_list(catalog, &hw, &measured, &rec, models_dir, a.sort);

    if let Some(out) = a.out {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = RunFile {
            hardware: hw,
            reports,
            failures,
            recommendation: rec,
        };
        std::fs::write(&out, serde_json::to_string_pretty(&file)?)?;
        println!("\nSaved {}", out.display());
    }
    Ok(())
}

fn print_table(catalog: &Catalog, reports: &[WorkerReport]) {
    let sets: Vec<String> = reports
        .first()
        .map(|r| r.scores.iter().map(|s| s.set.clone()).collect())
        .unwrap_or_default();
    println!();
    print!(
        "{:<7} {:<42} {:>6} {:>8} {:>7} {:>7}",
        "Pass", "Model", "RTF", "p95 ms", "RAM MB", "Load s"
    );
    for s in &sets {
        print!(" {:>11}", format!("WER {}", s.trim_start_matches("test-")));
    }
    println!();
    for role in [Role::Live, Role::Refine] {
        for r in reports {
            let Some(m) = catalog.get(&r.speed.model_id) else {
                continue;
            };
            if m.role != role {
                continue;
            }
            print!(
                "{:<7} {:<42} {:>6.3} {:>8} {:>7} {:>7.1}",
                format!("{role:?}"),
                m.name,
                r.speed.rtf,
                r.speed
                    .chunk_p95_ms
                    .map(|v| format!("{v:.0}"))
                    .unwrap_or_else(|| "-".into()),
                r.speed.peak_rss_mb,
                r.speed.load_ms as f32 / 1000.0
            );
            for s in &r.scores {
                print!(" {:>10.2}%", s.wer);
            }
            println!();
        }
    }
}

/// Recommendation from measured results plus estimates for models not timed here.
fn recommend_with_estimates(
    hw: &HardwareReport,
    catalog: &Catalog,
    measured: &[SpeedResult],
) -> Recommendation {
    let factor = listing::calibration_factor(catalog, measured);
    let speeds: Vec<_> =
        listing::effective_speeds(catalog, measured, factor, hw.threads_per_pass())
            .into_iter()
            .map(|(r, _)| r)
            .collect();
    recommend::recommend(hw, catalog, &speeds, Limits::default())
}

/// Terminal preview of the Settings → Engine & Models list, coloured like the app.
fn print_model_list(
    catalog: &Catalog,
    hw: &HardwareReport,
    measured: &[SpeedResult],
    rec: &Recommendation,
    models_dir: &Path,
    sort: SortBy,
) {
    use std::io::IsTerminal;
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, text: &str| {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };

    let factor = listing::calibration_factor(catalog, measured);
    let speeds = listing::effective_speeds(catalog, measured, factor, hw.threads_per_pass());
    let mut rows = listing::rows(
        catalog,
        &speeds,
        rec,
        |m| m.is_installed(models_dir),
        Limits::default(),
    );
    listing::sort_rows(&mut rows, sort);

    println!();
    match factor {
        Some(f) => println!(
            "This PC runs speech models {:.1}x {} than the reference PC (Intel N150).",
            if f > 1.0 { f } else { 1.0 / f },
            if f > 1.0 { "slower" } else { "faster" }
        ),
        None => println!("No calibration result; speeds unknown."),
    }
    for (role, title) in [(Role::Live, "Live captions"), (Role::Refine, "Correction")] {
        println!("\n{} — sorted by: {}", paint("1", title), sort.label());
        for r in rows.iter().filter(|r| r.role == role) {
            let fit = match r.fit {
                Fit::Smooth => paint("32", r.fit.label()),
                Fit::MayLag => paint("33", r.fit.label()),
                Fit::TooSlow => paint("31", r.fit.label()),
                Fit::Unknown => paint("2", r.fit.label()),
            };
            let speed = match (r.rtf, r.speed_source) {
                (Some(v), SpeedSource::Measured) => format!("{:.1}x real time", 1.0 / v),
                (Some(v), _) => format!("~{:.1}x real time", 1.0 / v),
                (None, _) => "speed ?".into(),
            };
            println!(
                "  {} {:<34} {:>15}  {:>16}  {:>7}  {:>6} MB  {}  {}",
                if r.recommended {
                    paint("32;1", "★")
                } else {
                    " ".into()
                },
                r.name,
                r.accuracy_pct
                    .map(|a| format!("{a:.1}% accurate"))
                    .unwrap_or_else(|| "accuracy ?".into()),
                speed,
                r.ram_mb.map(|m| format!("{m} MB")).unwrap_or_default(),
                r.download_mb,
                if r.installed {
                    paint("36", "installed ")
                } else {
                    "download  ".into()
                },
                fit,
            );
            println!("    {}", paint("2", &r.summary));
        }
    }
    println!(
        "\n★ recommended · ~ estimated from calibration · RAM is peak while running · MB column = download size"
    );
}

fn print_recommendation(rec: &Recommendation) {
    println!("\nRecommended for this PC ({:?} tier):", rec.tier);
    match &rec.live {
        Some(c) => println!(
            "  Live captions : {} on {} ({} threads, RTF {:.2}, {} MB)",
            c.model_name, c.provider, c.threads, c.rtf, c.peak_rss_mb
        ),
        None => println!("  Live captions : none available"),
    }
    match &rec.refine {
        Some(c) => println!(
            "  Correction    : {} on {} ({} threads, RTF {:.2}, {} MB)",
            c.model_name, c.provider, c.threads, c.rtf, c.peak_rss_mb
        ),
        None => println!("  Correction    : off"),
    }
    for n in &rec.notes {
        println!("  Note: {n}");
    }
}

struct Utt {
    reference: String,
    samples: Vec<f32>,
}

fn load_set(data: &Path, set: &str, max: Option<usize>) -> Result<Vec<Utt>> {
    let dir = data.join(set);
    let refs = std::fs::read_to_string(dir.join("refs.tsv")).with_context(|| {
        format!(
            "missing test set {} (run scripts/fetch-bench-assets.sh)",
            dir.display()
        )
    })?;
    let lines: Vec<(&str, &str)> = refs.lines().filter_map(|l| l.split_once('\t')).collect();
    // Spread the picks evenly so many speakers are covered.
    let n = max.unwrap_or(lines.len()).min(lines.len());
    let picks = (0..n).map(|i| lines[i * lines.len() / n]);
    picks
        .map(|(id, text)| {
            let path = dir.join("wav").join(format!("{id}.wav"));
            let wave = sherpa_onnx::Wave::read(&path.to_string_lossy())
                .with_context(|| format!("reading {}", path.display()))?;
            if wave.sample_rate() != asr::SAMPLE_RATE {
                bail!(
                    "{} is {} Hz, expected 16000",
                    path.display(),
                    wave.sample_rate()
                );
            }
            Ok(Utt {
                reference: text.to_string(),
                samples: wave.samples().to_vec(),
            })
        })
        .collect()
}

fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn worker(
    catalog: &Catalog,
    models_dir: &Path,
    id: &str,
    sets: &[String],
    data: &Path,
    max_utts: Option<usize>,
    threads: i32,
    provider: &str,
) -> Result<WorkerReport> {
    let m = catalog
        .get(id)
        .with_context(|| format!("unknown model {id}"))?;
    if !m.is_installed(models_dir) {
        bail!("model {id} is not installed in {}", models_dir.display());
    }
    let t = Instant::now();
    let rec = asr::build(m, models_dir, threads, provider)?;
    let load_ms = t.elapsed().as_millis() as u64;

    // Warm-up so one-time allocations don't count against the first utterance.
    rec.transcribe(m, &vec![0.0; asr::SAMPLE_RATE as usize])?;

    let mut scores = vec![];
    let mut samples = vec![];
    let (mut total_audio, mut total_compute) = (0f64, 0f64);
    let mut chunk_ms: Vec<f32> = vec![];

    for set in sets {
        let utts = load_set(data, set, max_utts)?;
        let (mut ref_words, mut errs, mut audio_s) = (0usize, 0usize, 0f32);
        for u in &utts {
            let t = Instant::now();
            let out = rec.transcribe(m, &u.samples)?;
            total_compute += t.elapsed().as_secs_f64();
            let dur = u.samples.len() as f32 / asr::SAMPLE_RATE as f32;
            audio_s += dur;
            chunk_ms.extend(out.chunk_times.iter().map(|d| d.as_secs_f32() * 1000.0));

            let r = wer::normalize(&u.reference);
            let h = wer::normalize(&out.text);
            ref_words += r.len();
            errs += wer::errors(&r, &h);
            if samples.len() < 4 {
                samples.push((u.reference.clone(), out.text.trim().to_string()));
            }
        }
        total_audio += audio_s as f64;
        scores.push(SetScore {
            set: set.clone(),
            utts: utts.len(),
            audio_s,
            ref_words,
            errors: errs,
            wer: 100.0 * errs as f32 / ref_words.max(1) as f32,
        });
    }

    let chunk_p95_ms = (!chunk_ms.is_empty()).then(|| {
        chunk_ms.sort_by(f32::total_cmp);
        chunk_ms[((chunk_ms.len() - 1) as f32 * 0.95) as usize]
    });

    Ok(WorkerReport {
        speed: SpeedResult {
            model_id: id.into(),
            provider: provider.into(),
            threads,
            rtf: (total_compute / total_audio.max(1e-6)) as f32,
            peak_rss_mb: peak_rss_mb(),
            load_ms,
            chunk_p95_ms,
            failed: None,
        },
        scores,
        samples,
    })
}
