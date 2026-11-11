//! Builds sherpa-onnx recognizers from catalogue entries and transcribes audio.

use anyhow::{Result, anyhow, bail};
use captionist_core::catalog::{Engine, ModelEntry};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OnlineRecognizer, OnlineRecognizerConfig,
};
use std::path::Path;
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: i32 = 16_000;
/// Audio is fed to streaming models in 100 ms pieces, like live capture.
pub const CHUNK: usize = 1_600;

pub enum Recognizer {
    Online(OnlineRecognizer),
    Offline(OfflineRecognizer),
}

pub struct Transcript {
    pub text: String,
    /// Streaming only: compute time for each 100 ms chunk.
    pub chunk_times: Vec<Duration>,
}

/// ONNX Runtime threads normally spin-wait between jobs, keeping cores busy
/// while idle. For live captions that doubled CPU use, so plain "cpu" gets a
/// config that turns spinning off.
fn cpu_provider() -> String {
    let path = crate::paths::state_dir().join("cpu-nospin.conf");
    let body = "SessionConfig.session.intra_op.allow_spinning=0\nSessionConfig.session.inter_op.allow_spinning=0\n";
    let ok = std::fs::create_dir_all(crate::paths::state_dir()).is_ok()
        && (std::fs::read_to_string(&path).is_ok_and(|s| s == body)
            || std::fs::write(&path, body).is_ok());
    if ok {
        format!("cpu:{}", path.display())
    } else {
        "cpu".into()
    }
}

pub fn build(m: &ModelEntry, root: &Path, threads: i32, provider: &str) -> Result<Recognizer> {
    let provider = if provider == "cpu" {
        cpu_provider()
    } else {
        provider.to_string()
    };
    let provider = provider.as_str();
    let f = &m.files;
    let p = |file: &Option<String>| m.path(root, file);
    match m.engine {
        Engine::OnlineTransducer => {
            let mut c = OnlineRecognizerConfig::default();
            c.model_config.transducer.encoder = p(&f.encoder);
            c.model_config.transducer.decoder = p(&f.decoder);
            c.model_config.transducer.joiner = p(&f.joiner);
            c.model_config.tokens = p(&f.tokens);
            c.model_config.num_threads = threads;
            c.model_config.provider = Some(provider.into());
            c.model_config.model_type = m.model_type.clone();
            c.decoding_method = Some("greedy_search".into());
            OnlineRecognizer::create(&c)
                .map(Recognizer::Online)
                .ok_or_else(|| anyhow!("sherpa-onnx could not load {}", m.id))
        }
        Engine::OfflineTransducer | Engine::OfflineMoonshine | Engine::OfflineWhisper => {
            let mut c = OfflineRecognizerConfig::default();
            let mc = &mut c.model_config;
            match m.engine {
                Engine::OfflineTransducer => {
                    mc.transducer.encoder = p(&f.encoder);
                    mc.transducer.decoder = p(&f.decoder);
                    mc.transducer.joiner = p(&f.joiner);
                }
                Engine::OfflineMoonshine => {
                    mc.moonshine.preprocessor = p(&f.preprocessor);
                    mc.moonshine.encoder = p(&f.encoder);
                    mc.moonshine.uncached_decoder = p(&f.uncached_decoder);
                    mc.moonshine.cached_decoder = p(&f.cached_decoder);
                    mc.moonshine.merged_decoder = p(&f.merged_decoder);
                }
                Engine::OfflineWhisper => {
                    mc.whisper.encoder = p(&f.encoder);
                    mc.whisper.decoder = p(&f.decoder);
                    mc.whisper.language = Some("en".into());
                    mc.whisper.task = Some("transcribe".into());
                    mc.whisper.tail_paddings = -1;
                }
                Engine::OnlineTransducer => unreachable!(),
            }
            mc.tokens = p(&f.tokens);
            mc.num_threads = threads;
            mc.provider = Some(provider.into());
            mc.model_type = m.model_type.clone();
            c.decoding_method = Some("greedy_search".into());
            OfflineRecognizer::create(&c)
                .map(Recognizer::Offline)
                .ok_or_else(|| anyhow!("sherpa-onnx could not load {}", m.id))
        }
    }
}

impl Recognizer {
    pub fn transcribe(&self, m: &ModelEntry, samples: &[f32]) -> Result<Transcript> {
        match self {
            Recognizer::Online(rec) => {
                let s = rec.create_stream();
                for (k, v) in &m.stream_options {
                    s.set_option(k, v);
                }
                let mut chunk_times = Vec::with_capacity(samples.len() / CHUNK + 1);
                for chunk in samples.chunks(CHUNK) {
                    let t = Instant::now();
                    s.accept_waveform(SAMPLE_RATE, chunk);
                    while rec.is_ready(&s) {
                        rec.decode(&s);
                    }
                    chunk_times.push(t.elapsed());
                }
                // Silence at the end flushes the last words out of the model.
                let tail = vec![0.0f32; (m.tail_padding_s * SAMPLE_RATE as f32) as usize];
                s.accept_waveform(SAMPLE_RATE, &tail);
                s.input_finished();
                while rec.is_ready(&s) {
                    rec.decode(&s);
                }
                let text = rec.get_result(&s).map(|r| r.text).unwrap_or_default();
                Ok(Transcript { text, chunk_times })
            }
            Recognizer::Offline(rec) => {
                let s = rec.create_stream();
                s.accept_waveform(SAMPLE_RATE, samples);
                rec.decode(&s);
                let Some(r) = s.get_result() else {
                    bail!("no result from {}", m.id)
                };
                Ok(Transcript {
                    text: r.text,
                    chunk_times: vec![],
                })
            }
        }
    }
}
