#!/usr/bin/env bash
# ONE-TIME, after the GitHub repository exists: labels, milestones, Discussions,
# private vulnerability reporting, merge settings and protection for main.
#
#   scripts/github/setup-repo.sh [owner/repo]   (default: m-umair-khan-dev/captionist)
set -euo pipefail
REPO="${1:-m-umair-khan-dev/captionist}"

echo "==> Repository settings"
gh repo edit "$REPO" \
  --description "Private, offline live captions for Linux and GNOME: captions videos, calls and your mic in real time. Open-source alternative to Windows Live Captions." \
  --homepage "https://github.com/$REPO#readme" \
  --enable-discussions --enable-issues --enable-wiki=false \
  --enable-squash-merge --enable-merge-commit=false --enable-rebase-merge=false \
  --delete-branch-on-merge \
  --add-topic live-captions --add-topic speech-to-text --add-topic subtitles \
  --add-topic accessibility --add-topic gnome --add-topic gnome-shell-extension \
  --add-topic linux --add-topic offline --add-topic privacy --add-topic rust \
  --add-topic asr --add-topic transcription --add-topic deaf --add-topic hard-of-hearing

echo "==> Squash commits use the PR title (it becomes the changelog entry)"
gh api -X PATCH "repos/$REPO" -f squash_merge_commit_title=PR_TITLE -f squash_merge_commit_message=PR_BODY >/dev/null

echo "==> Let workflows open release pull requests (release-please)"
gh api -X PUT "repos/$REPO/actions/permissions/workflow" -f default_workflow_permissions=read -F can_approve_pull_request_reviews=true >/dev/null

echo "==> Private vulnerability reporting"
gh api -X PUT "repos/$REPO/private-vulnerability-reporting" >/dev/null

echo "==> Labels"
label() { gh label create "$1" --repo "$REPO" --color "$2" --description "$3" --force >/dev/null; }
label bug              d73a4a "Something isn't working"
label feature          a2eeef "New feature or request"
label needs-triage     ededed "Not yet looked at"
label "good first issue" 7057ff "Good for newcomers"
label "help wanted"    008672 "Extra attention is needed"
label accuracy         fbca04 "Recognition quality"
label models           1d76db "Speech models and downloads"
label extension        5319e7 "GNOME Shell extension: caption box, top bar"
label settings         0e8a16 "Settings window"
label acceleration     c5def5 "GPU / NPU acceleration"
label packaging        bfd4f2 "deb, APT repository, install/update"
label languages        f9d0c4 "Non-English support"

echo "==> Milestones"
ms() { gh api "repos/$REPO/milestones" -f title="$1" -f description="$2" >/dev/null 2>&1 || true; }
ms "0.2.0" "Polish from first user feedback"
ms "0.3.0" "More languages (Parakeet v3 / Nemotron multilingual)"
ms "0.4.0" "Offline translation"

echo "==> Protect main (PRs + green CI required)"
gh api -X PUT "repos/$REPO/branches/main/protection" --input - >/dev/null <<'EOF'
{
  "required_status_checks": { "strict": true, "contexts": ["Build, lint and test"] },
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
EOF
echo "Done: $REPO"
