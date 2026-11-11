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

Pull requests are squash-merged, so **the PR title becomes the commit message**
on `main`. Keep it short (one line, at most two).

| Change | Title | Release |
|---|---|---|
| Bug fix | Starts with **`fix:`**, e.g. `fix: caption box hides while the menu is open` | Triggers a patch release (0.2.0 → 0.2.1) and a changelog entry |
| Anything else (features, docs, CI, refactoring) | No prefix, e.g. `Add speaker labels to history export` | Shipped in the next release; maintainers choose when a feature release (0.3.0) happens |

## How releases reach users

Maintainers: see [docs/RELEASING.md](docs/RELEASING.md). In short, merging to
`main` keeps a release pull request up to date; merging that PR tags the
version, builds the `.deb`, publishes a GitHub Release and updates the signed
APT repository, so users get the update with their normal system updates.
