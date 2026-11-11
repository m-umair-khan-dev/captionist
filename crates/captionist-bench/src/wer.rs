//! Word error rate: (substitutions + deletions + insertions) / reference words.

/// Upper-case, drop punctuation (keeping apostrophes inside words), split on spaces.
pub fn normalize(text: &str) -> Vec<String> {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c.to_ascii_uppercase()
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Word-level edit distance.
pub fn errors(reference: &[String], hypothesis: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut cur = vec![0; hypothesis.len() + 1];
    for (i, r) in reference.iter().enumerate() {
        cur[0] = i + 1;
        for (j, h) in hypothesis.iter().enumerate() {
            let sub = prev[j] + usize::from(r != h);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[hypothesis.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_edits() {
        let r = normalize("THE CAT SAT ON THE MAT");
        assert_eq!(errors(&r, &normalize("The cat sat on the mat.")), 0);
        assert_eq!(errors(&r, &normalize("the cat sat on mat")), 1);
        assert_eq!(errors(&r, &normalize("a cat sat on the big mat")), 2);
        assert_eq!(normalize("Don't stop—now!"), vec!["DON'T", "STOP", "NOW"]);
    }
}
