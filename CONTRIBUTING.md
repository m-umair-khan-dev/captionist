# Contributing to Captionist

Thanks for helping! Bug reports, translations, testing on different hardware and
code are all welcome.

- **Questions and ideas:** [Discussions](https://github.com/m-umair-khan-dev/captionist/discussions)
- **Bugs and feature requests:** [Issues](https://github.com/m-umair-khan-dev/captionist/issues/new/choose); the forms ask for what's needed
- **Security problems:** please report privately, see [SECURITY.md](SECURITY.md)
- **Good places to start:** issues labelled [`good first issue`](https://github.com/m-umair-khan-dev/captionist/labels/good%20first%20issue)

## Making a change

1. Fork the repository and create a branch: `git switch -c fix/caption-box-hides-early`.
2. Build and test; everything runs in Docker, no local Rust toolchain needed
   (details in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)):
   ```sh
   scripts/build-sherpa.sh                     # once, ~20 min
   scripts/cargo-docker.sh fmt --all
   scripts/cargo-docker.sh clippy --release --workspace --all-targets
   scripts/cargo-docker.sh test --release --workspace
   scripts/build-deb.sh && sudo apt install ./dist/captionist_*.deb   # try it for real
   ```
3. Open a pull request. CI builds, lints and tests it and attaches a `.deb` you
   (and reviewers) can install from the run's summary page.

## Commit messages and PR titles

We use [Conventional Commits](https://www.conventionalcommits.org/). Pull requests
are squash-merged, so **the PR title becomes the changelog entry** and decides the
next version number:

| Prefix | Use for | Version bump (while 0.x) |
|---|---|---|
| `fix:` | A bug fix | patch: 0.2.0 → 0.2.1 |
| `feat:` | A new user-visible feature | minor: 0.2.1 → 0.3.0 |
| `feat!:` or `BREAKING CHANGE:` | Incompatible change (settings, IDs…) | minor while 0.x, major from 1.0 |
| `docs:` `ci:` `chore:` `refactor:` `test:` `perf:` | Everything else | none on its own |

Examples: `fix: caption box hides while the menu is open`,
`feat: speaker labels in history export`.

## How releases reach users

Maintainers: see [docs/RELEASING.md](docs/RELEASING.md). In short, merging to
`main` keeps a release pull request up to date; merging that PR tags the
version, builds the `.deb`, publishes a GitHub Release and updates the signed
APT repository, so users get the update with their normal system updates.
