# Releasing Captionist (maintainers)

## How it works

```
 PR ("fix: …" or plain title) ──squash-merge──▶ main
                                         │  release-please
                                         ▼
                     "chore(main): release 0.2.1" PR   ← keeps updating as more PRs land
                     (Cargo.toml version + CHANGELOG.md)
                                         │  you merge it
                                         ▼
                 tag v0.2.1 + GitHub Release
                                         │  .github/workflows/release.yml
                                         ▼
       build .deb → attach .deb + SHA256SUMS to the release
                                         │
                                         ▼
       sign + publish to the APT repository (gh-pages branch → GitHub Pages)
                                         │
                                         ▼
       users: normal system updates (incl. unattended-upgrades) install it;
       the extension then asks them to log out once to finish
```

- **Version source:** `Cargo.toml` (`[workspace.package] version`, marked
  `# x-release-please-version`). The `.deb` and the extension's version are stamped from it.
- **Tags:** `vX.Y.Z`, created only by release-please; never move or delete a tag.
- **Changelog:** `CHANGELOG.md`, generated from `fix:` PR titles (see CONTRIBUTING.md).
- **Feature releases:** add the line `Release-As: 0.X.0` to a commit message to bump the minor version.

## One-time setup (before the first release)

1. **APT signing key.** On a trusted machine:
   ```sh
   scripts/apt-repo/new-signing-key.sh
   gh secret set APT_SIGNING_KEY < captionist-apt-signing-key.asc
   # back the private key up offline, then:
   shred -u captionist-apt-signing-key.asc
   git add packaging/apt/captionist.asc   # the public key is committed
   ```
2. **Repository settings:** run `scripts/github/setup-repo.sh` once (labels,
   milestones, Discussions, private vulnerability reporting, branch protection,
   squash merges).
3. **GitHub Pages:** Settings → Pages → Source: *Deploy from a branch*,
   branch `gh-pages`, folder `/ (root)`. Available after the first release run
   creates the branch.

## The first release (v0.1.0)

release-please starts from the version in `.release-please-manifest.json`
(0.1.0), so the first tag is made by hand:

```sh
git tag -a v0.1.0 -m "Captionist 0.1.0"
git push origin v0.1.0
gh workflow run release.yml -f tag=v0.1.0   # builds, attaches the .deb, publishes the APT repo
```

Every later release goes through the release PR.

## A hotfix

Merge the `fix:` PR, then merge the release PR it updates (e.g. 0.2.0 → 0.2.1).
That's all: users receive it with their next system update.

## If a release is bad

Don't delete the tag. Revert the change with a `fix:` PR and release again
(0.2.2). The APT repository keeps the 5 newest versions, so users can also
downgrade: `sudo apt install captionist=0.2.0`.
