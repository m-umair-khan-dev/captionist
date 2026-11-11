//! Caption text clean-up.

/// Tidy model output: trim, collapse spaces, sentence-case all-caps output.
pub fn clean(raw: &str) -> String {
    let s = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let has_lower = s.chars().any(|c| c.is_lowercase());
    let has_upper = s.chars().any(|c| c.is_uppercase());
    if has_upper && !has_lower {
        // Some models output ALL CAPS; make it readable.
        let lower = s.to_lowercase();
        let mut out = String::with_capacity(lower.len());
        let mut cap_next = true;
        for word in lower.split(' ') {
            if !out.is_empty() {
                out.push(' ');
            }
            let w = match word {
                "i" => "I".to_string(),
                w if w.starts_with("i'") => format!("I{}", &w[1..]),
                w if cap_next => {
                    let mut c = w.chars();
                    c.next()
                        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                }
                w => w.to_string(),
            };
            cap_next = w.ends_with(['.', '?', '!']);
            out.push_str(&w);
        }
        return out;
    }
    s
}

const PROFANITY: &[&str] = &[
    "fuck",
    "fucking",
    "fucked",
    "fucker",
    "shit",
    "shitty",
    "bitch",
    "bastard",
    "asshole",
    "dick",
    "cunt",
    "motherfucker",
    "bullshit",
    "piss",
    "pissed",
    "slut",
    "whore",
];

/// Replace swear words with their first letter plus asterisks.
pub fn mask_profanity(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let core: String = w
                .chars()
                .filter(|c| c.is_alphabetic())
                .collect::<String>()
                .to_lowercase();
            if PROFANITY.contains(&core.as_str()) {
                w.chars()
                    .enumerate()
                    .map(|(i, c)| if i > 0 && c.is_alphabetic() { '*' } else { c })
                    .collect()
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ───────────────────────── Custom vocabulary ─────────────────────────

/// One caption word split into leading punctuation, the word, trailing punctuation.
struct Token {
    pre: String,
    core: String,
    post: String,
}

fn tokenize(s: &str) -> Vec<Token> {
    s.split_whitespace()
        .map(|w| {
            let start = w.find(|c: char| c.is_alphanumeric()).unwrap_or(w.len());
            let end = w
                .rfind(|c: char| c.is_alphanumeric())
                .map(|i| i + w[i..].chars().next().unwrap().len_utf8())
                .unwrap_or(start);
            Token {
                pre: w[..start].to_string(),
                core: w[start..end.max(start)].to_string(),
                post: w[end.max(start)..].to_string(),
            }
        })
        .collect()
}

/// Lower-case letters and digits only: "Kuber-Netes" → "kubernetes".
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Sound-alike code (Soundex groups, no length limit): words that sound the
/// same get the same code, e.g. "Usman"/"Osman", "Kubernetes"/"cooper netties".
fn sound_code(k: &str) -> String {
    let group = |c: char| match c {
        'b' | 'f' | 'p' | 'v' => '1',
        'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => '2',
        'd' | 't' => '3',
        'l' => '4',
        'm' | 'n' => '5',
        'r' => '6',
        _ => '0',
    };
    let mut out = String::new();
    let mut last = ' ';
    for (i, c) in k.chars().enumerate() {
        let g = group(c);
        if i == 0 {
            out.push(if g == '0' { 'A' } else { g });
        } else if g != '0' && g != last {
            out.push(g);
        }
        last = g;
    }
    out
}

fn similarity(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for j in 0..b.len() {
            cur[j + 1] = (prev[j] + usize::from(*ca != b[j]))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    1.0 - prev[b.len()] as f32 / a.len().max(b.len()).max(1) as f32
}

struct VocabEntry {
    display: String,
    key: String,
    code: String,
    words: usize,
}

/// User-tunable caption post-processing: custom vocabulary, replacements and
/// the profanity filter. Shared with the pipeline and updated live.
#[derive(Default)]
pub struct PostProcessor {
    vocab: Vec<VocabEntry>,
    /// (heard words, lower-case) → replacement
    replacements: Vec<(Vec<String>, String)>,
    pub mask_profanity: bool,
}

impl PostProcessor {
    pub fn new(
        vocabulary: &[String],
        replacements: &[(String, String)],
        mask_profanity: bool,
    ) -> Self {
        let vocab = vocabulary
            .iter()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(|v| {
                let k = key(v);
                VocabEntry {
                    display: v.to_string(),
                    code: sound_code(&k),
                    key: k,
                    words: v.split_whitespace().count(),
                }
            })
            .collect();
        let replacements = replacements
            .iter()
            .filter(|(from, _)| !from.trim().is_empty())
            .map(|(from, to)| {
                (
                    from.split_whitespace().map(key).collect(),
                    to.trim().to_string(),
                )
            })
            .collect();
        PostProcessor {
            vocab,
            replacements,
            mask_profanity,
        }
    }

    /// Does `heard` (joined words, as a key) match vocabulary entry `e`?
    fn matches(e: &VocabEntry, heard: &str) -> bool {
        if heard == e.key {
            return true;
        }
        let len = e.key.chars().count();
        if len <= 3 {
            return false;
        }
        let sim = similarity(heard, &e.key);
        let same_sound = sound_code(heard) == e.code;
        // Sound-alike is required, so real words that merely look similar
        // ("headline" vs "Captionist") are left alone.
        same_sound && ((len >= 6 && sim >= 0.5) || sim >= 0.7)
    }

    pub fn apply(&self, text: &str) -> String {
        let mut toks = tokenize(text);

        // 1. Replacements: exact (case-insensitive) word sequences.
        if !self.replacements.is_empty() {
            let mut out: Vec<Token> = vec![];
            let mut i = 0;
            'outer: while i < toks.len() {
                for (from, to) in &self.replacements {
                    let n = from.len();
                    if i + n <= toks.len()
                        && toks[i..i + n]
                            .iter()
                            .zip(from)
                            .all(|(t, f)| key(&t.core) == *f)
                    {
                        out.push(Token {
                            pre: toks[i].pre.clone(),
                            core: to.clone(),
                            post: toks[i + n - 1].post.clone(),
                        });
                        i += n;
                        continue 'outer;
                    }
                }
                out.push(Token {
                    pre: toks[i].pre.clone(),
                    core: toks[i].core.clone(),
                    post: toks[i].post.clone(),
                });
                i += 1;
            }
            toks = out;
        }

        // 2. Vocabulary: windows of 1–4 words that sound like an entry.
        if !self.vocab.is_empty() {
            let mut out: Vec<Token> = vec![];
            let mut i = 0;
            while i < toks.len() {
                let mut best: Option<(usize, &VocabEntry)> = None;
                for n in (1..=4.min(toks.len() - i)).rev() {
                    // Don't merge across sentence punctuation.
                    if toks[i..i + n - 1]
                        .iter()
                        .any(|t| t.post.contains(['.', '?', '!', ',']))
                    {
                        continue;
                    }
                    let heard: String = toks[i..i + n].iter().map(|t| key(&t.core)).collect();
                    if heard.is_empty() {
                        continue;
                    }
                    if let Some(e) = self
                        .vocab
                        .iter()
                        .find(|e| e.words.abs_diff(n) <= 2 && Self::matches(e, &heard))
                    {
                        best = Some((n, e));
                        break;
                    }
                }
                match best {
                    Some((n, e)) => {
                        out.push(Token {
                            pre: toks[i].pre.clone(),
                            core: e.display.clone(),
                            post: toks[i + n - 1].post.clone(),
                        });
                        i += n;
                    }
                    None => {
                        out.push(Token {
                            pre: toks[i].pre.clone(),
                            core: toks[i].core.clone(),
                            post: toks[i].post.clone(),
                        });
                        i += 1;
                    }
                }
            }
            toks = out;
        }

        let s = toks
            .iter()
            .map(|t| format!("{}{}{}", t.pre, t.core, t.post))
            .collect::<Vec<_>>()
            .join(" ");
        if self.mask_profanity {
            mask_profanity(&s)
        } else {
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_text() {
        assert_eq!(
            clean("  HELLO  I'M HERE. HOW ARE YOU "),
            "Hello I'm here. How are you"
        );
        assert_eq!(clean("Already Fine."), "Already Fine.");
        assert_eq!(mask_profanity("well, shit happens"), "well, s*** happens");
    }

    #[test]
    fn vocabulary_fixes_names_and_jargon() {
        let p = PostProcessor::new(
            &[
                "Kubernetes".into(),
                "Usman".into(),
                "GNOME".into(),
                "Captionist".into(),
            ],
            &[("pipe wire".into(), "PipeWire".into())],
            false,
        );
        assert_eq!(
            p.apply("We deploy on cooper netties today."),
            "We deploy on Kubernetes today."
        );
        assert_eq!(p.apply("Hi Osman, how are you?"), "Hi Usman, how are you?");
        assert_eq!(p.apply("the gnome desktop"), "the GNOME desktop");
        assert_eq!(p.apply("It uses pipe wire."), "It uses PipeWire.");
        // Looks similar but sounds different: untouched.
        assert_eq!(p.apply("Read the headline."), "Read the headline.");
        assert_eq!(p.apply("A woman spoke."), "A woman spoke.");
    }
}
