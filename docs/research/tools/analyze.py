"""Pattern analysis of popular open-source READMEs (stdlib only).

Reads READMEs saved by the download step (treated purely as text data),
extracts structural and wording features, then:
  1. TF-IDF: words that distinguish the opening pitch (first ~250 words)
  2. Spearman correlation of each feature with log(stars)
  3. k-means (k=3) on normalised features → README archetypes
"""
import json, math, os, re, sys, random, urllib.request
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, '..', 'readmes')
repos = [l.split() for l in open(os.path.join(HERE, 'repos.txt')) if l.strip()]

# ── stars (GitHub API, cached) ──────────────────────────────────────────────
cache_p = os.path.join(HERE, 'stars.json')
stars = json.load(open(cache_p)) if os.path.exists(cache_p) else {}
for repo, _ in repos:
    if repo in stars:
        continue
    try:
        with urllib.request.urlopen(f'https://api.github.com/repos/{repo}', timeout=20) as r:
            stars[repo] = json.load(r)['stargazers_count']
    except Exception as e:
        print('stars failed', repo, e, file=sys.stderr)
json.dump(stars, open(cache_p, 'w'), indent=1)

STOP = set('''a an the and or of to in on for with is are be by as it its this that from at
your you we our can will not no all any into via use using used than then so if but more most
also just which their they them these those has have was were been one two how what when
https http www com org github md png svg img src href alt width height align center div p br
nbsp gt lt amp quot strong em code pre li ul ol td tr th table h1 h2 h3 en us'''.split())

PRIVACY = r'\b(privacy|private|offline|local(ly)?|on-device|no cloud|self-host(ed)?|your data|never leaves|no account|no telemetry)\b'
SPEED = r'\b(fast|faster|fastest|blazing|lightning|real-?time|instant(ly)?|lightweight|low[- ]latency|performance)\b'
EASE = r'\b(easy|easily|simple|simply|one[- ]click|just works|out of the box|effortless|drag and drop|no setup)\b'
FREE = r'\b(free|open[- ]source|foss|gpl|mit license|apache)\b'
EMOJI = re.compile('[\U0001F300-\U0001FAFF☀-➿]')

def strip_md(t):
    t = re.sub(r'<[^>]+>', ' ', t)
    t = re.sub(r'!\[[^\]]*\]\([^)]*\)', ' ', t)
    t = re.sub(r'\[([^\]]*)\]\([^)]*\)', r'\1', t)
    t = re.sub(r'`[^`]*`', ' ', t)
    return t

def tagline(text):
    for line in text.splitlines():
        s = strip_md(line).strip(' #>*_|-=\t')
        if len(s.split()) >= 4 and not s.lower().startswith(('build', 'license', 'download', 'table of')):
            return s
    return ''

def features(text):
    head = '\n'.join(text.splitlines()[:25])
    lower = text.lower()
    plain = strip_md(text).lower()
    words = re.findall(r"[a-z][a-z'-]+", plain)
    headings = [re.sub(r'[^a-z ]', '', h.lower()).strip() for h in re.findall(r'^#{1,3}\s+(.+)$', text, re.M)]
    imgs = re.findall(r'!\[[^\]]*\]\(([^)]+)\)|<img[^>]+src="([^"]+)"', text, re.I)
    imgs = [a or b for a, b in imgs]
    badges = [i for i in imgs if re.search(r'shields\.io|badge|badgen', i, re.I)]
    shots = [i for i in imgs if i not in badges and not re.search(r'logo|icon', i, re.I)]
    install_idx = next((i for i, h in enumerate(headings) if re.search(r'install|download|get started|getting started|quick ?start', h)), None)
    n = max(1, len(words))
    return {
        'words': len(words),
        'logo_top': int(bool(re.search(r'<img[^>]+(logo|icon)|!\[[^\]]*\]\([^)]*(logo|icon)', head, re.I))),
        'centered_header': int('align="center"' in head.lower() or "align='center'" in head.lower()),
        'badges': len(badges),
        'screenshots': len(shots),
        'gif_or_video': int(bool(re.search(r'\.gif|\.mp4|\.webm|youtube\.com|youtu\.be', lower))),
        'headings': len(headings),
        'install_pos': (install_idx / max(1, len(headings))) if install_idx is not None else 1.0,
        'has_features_section': int(any('feature' in h for h in headings)),
        'has_why_section': int(any(h.startswith('why') for h in headings)),
        'has_faq': int(any('faq' in h or 'question' in h for h in headings)),
        'has_comparison_table': int(bool(re.search(r'\|.*(vs|versus|compar).*\|', lower))),
        'bullets': len(re.findall(r'^\s*[-*]\s+', text, re.M)),
        'emoji_per_1k': 1000 * len(EMOJI.findall(text)) / n,
        'you_per_1k': 1000 * sum(w in ('you', 'your', "you're") for w in words) / n,
        'privacy_per_1k': 1000 * len(re.findall(PRIVACY, plain)) / n,
        'speed_per_1k': 1000 * len(re.findall(SPEED, plain)) / n,
        'ease_per_1k': 1000 * len(re.findall(EASE, plain)) / n,
        'free_per_1k': 1000 * len(re.findall(FREE, plain)) / n,
        'tagline_words': len(tagline(text).split()),
    }, headings, tagline(text), words

rows = []
for repo, cat in repos:
    p = os.path.join(DATA, repo.replace('/', '_') + '.md')
    if not os.path.exists(p):
        continue
    text = open(p, encoding='utf-8', errors='replace').read()
    f, heads, tag, words = features(text)
    rows.append(dict(repo=repo, cat=cat, stars=stars.get(repo, 0), f=f, heads=heads, tag=tag, words=words))

rows.sort(key=lambda r: -r['stars'])
keys = list(rows[0]['f'])

def rank(v):
    order = sorted(range(len(v)), key=lambda i: v[i])
    r = [0.0] * len(v)
    i = 0
    while i < len(v):
        j = i
        while j + 1 < len(v) and v[order[j + 1]] == v[order[i]]:
            j += 1
        for k in range(i, j + 1):
            r[order[k]] = (i + j) / 2
        i = j + 1
    return r

def spearman(a, b):
    ra, rb = rank(a), rank(b)
    ma, mb = sum(ra) / len(ra), sum(rb) / len(rb)
    cov = sum((x - ma) * (y - mb) for x, y in zip(ra, rb))
    va = math.sqrt(sum((x - ma) ** 2 for x in ra)); vb = math.sqrt(sum((y - mb) ** 2 for y in rb))
    return cov / (va * vb) if va and vb else 0.0

out = []
P = lambda *a: out.append(' '.join(str(x) for x in a))

P(f'## Sample: {len(rows)} READMEs, stars {min(r["stars"] for r in rows):,} – {max(r["stars"] for r in rows):,}\n')
P('| repo | stars | cat | tagline |'); P('|---|---|---|---|')
for r in rows:
    P(f'| {r["repo"]} | {r["stars"]:,} | {r["cat"]} | {r["tag"][:110]} |')

P('\n## Feature prevalence (share of READMEs / median)\n')
for k in keys:
    v = [r['f'][k] for r in rows]
    if set(v) <= {0, 1}:
        P(f'- {k}: {100*sum(v)/len(v):.0f}% of READMEs')
    else:
        s = sorted(v); P(f'- {k}: median {s[len(s)//2]:.1f}')

P('\n## Spearman correlation with log(stars)  (n is small: indicative only)\n')
ls = [math.log10(max(1, r['stars'])) for r in rows]
cors = sorted(((spearman([r['f'][k] for r in rows], ls), k) for k in keys), reverse=True)
for c, k in cors:
    P(f'- {k}: {c:+.2f}')

P('\n## Most common section headings\n')
hc = Counter(h for r in rows for h in set(r['heads']) if h)
for h, c in hc.most_common(25):
    P(f'- {h}: {c}')

# TF-IDF of the opening pitch (first 250 words) vs whole-corpus document frequency
docs = [[w for w in r['words'] if w not in STOP and len(w) > 2] for r in rows]
pitch = [d[:250] for d in docs]
df = Counter(w for d in docs for w in set(d))
N = len(docs)
score = Counter()
for d in pitch:
    tf = Counter(d)
    for w, c in tf.items():
        score[w] += (c / max(1, len(d))) * math.log((1 + N) / (1 + df[w]))
P('\n## TF-IDF: most distinctive words in opening pitches\n')
P(', '.join(w for w, _ in score.most_common(45)))

# Pitch vocabulary used by the most-starred half vs the rest
half = len(rows) // 2
top_words = Counter(w for d in pitch[:half] for w in set(d))
low_words = Counter(w for d in pitch[half:] for w in set(d))
lift = sorted(((top_words[w] - low_words[w], w) for w in top_words if top_words[w] >= 4), reverse=True)
P('\n## Pitch words used more by the top-starred half\n')
P(', '.join(f'{w} (+{d})' for d, w in lift[:25]))

# k-means on z-scored features
X = [[r['f'][k] for k in keys] for r in rows]
mu = [sum(c) / len(c) for c in zip(*X)]
sd = [math.sqrt(sum((x - m) ** 2 for x in c) / len(c)) or 1 for c, m in zip(zip(*X), mu)]
Z = [[(x - m) / s for x, m, s in zip(row, mu, sd)] for row in X]
random.seed(7)
best = None
for _ in range(20):
    C = random.sample(Z, 3)
    for _ in range(50):
        lab = [min(range(3), key=lambda j: sum((a - b) ** 2 for a, b in zip(z, C[j]))) for z in Z]
        C = [[sum(z[i] for z, l in zip(Z, lab) if l == j) / max(1, lab.count(j)) for i in range(len(keys))] for j in range(3)]
    inertia = sum(sum((a - b) ** 2 for a, b in zip(z, C[l])) for z, l in zip(Z, lab))
    if best is None or inertia < best[0]:
        best = (inertia, lab, C)
_, lab, C = best
P('\n## k-means archetypes (k=3, z-scored features)\n')
for j in range(3):
    members = [r for r, l in zip(rows, lab) if l == j]
    if not members:
        continue
    traits = sorted(((C[j][i], keys[i]) for i in range(len(keys))), reverse=True)
    med = sorted(r['stars'] for r in members)[len(members) // 2]
    P(f'### Cluster {j}: {len(members)} repos, median stars {med:,}')
    P('high: ' + ', '.join(f'{k} ({v:+.1f}σ)' for v, k in traits[:5]))
    P('low: ' + ', '.join(f'{k} ({v:+.1f}σ)' for v, k in traits[-3:]))
    P('repos: ' + ', '.join(r['repo'] for r in members))
    P('')

open(os.path.join(HERE, 'report.md'), 'w').write('\n'.join(out))
print('\n'.join(out))
