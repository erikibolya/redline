#!/usr/bin/env bash
# Mark the current feature branch as ready for review.
#
#   bash desktop/scripts/feature-ready.sh "one-line summary"
#
# Pushes the branch, finds (or opens) the PR into `dev`, marks it ready and lets the
# auto-merge workflow take over. It never merges locally and never force-pushes.
set -euo pipefail

base="${BASE_BRANCH:-dev}"
summary="${1:-}"
root="$(git rev-parse --show-toplevel)"
cd "$root"

branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [ -z "$branch" ]; then
  echo "feature-ready: detached HEAD; check out a feature branch first." >&2
  exit 1
fi
case "$branch" in
  main|master|"$base")
    echo "feature-ready: refusing to mark $branch ready; switch to a feature branch." >&2
    exit 1
    ;;
esac
if [ -n "$(git status --porcelain)" ]; then
  echo "feature-ready: the working tree is dirty; commit or stash first." >&2
  exit 1
fi

# gh is installed on the host tooling path; the agent container has no ambient gh config.
if [ -z "${GH_CONFIG_DIR:-}" ] && [ -d /mnt/user/appdata/agent-tools/gh ]; then
  export GH_CONFIG_DIR=/mnt/user/appdata/agent-tools/gh
fi
if ! command -v gh >/dev/null 2>&1; then
  echo "feature-ready: gh is not installed." >&2
  exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
  echo "feature-ready: gh is not authenticated. Set GH_CONFIG_DIR to the gh config directory." >&2
  exit 1
fi

# Push the branch first: a PR needs the commits on GitHub. A no-op when it is already current.
if git rev-parse --abbrev-ref --symbolic-full-name '@{u}' >/dev/null 2>&1; then
  git push origin "$branch"
else
  git push -u origin "$branch"
fi

pr_number="$(gh pr list --head "$branch" --base "$base" --state open --json number --jq '.[0].number // empty')"
if [ -z "$pr_number" ]; then
  body="${summary:-Feature branch marked ready by RedLine.}"
  gh pr create \
    --base "$base" \
    --head "$branch" \
    --title "$(git log -1 --pretty=%s)" \
    --body "$body" >/dev/null
  pr_number="$(gh pr list --head "$branch" --base "$base" --state open --json number --jq '.[0].number // empty')"
fi
if [ -z "$pr_number" ]; then
  echo "feature-ready: could not find or create a PR for $branch." >&2
  exit 1
fi

if [ "$(gh pr view "$pr_number" --json isDraft --jq '.isDraft')" = "true" ]; then
  gh pr ready "$pr_number"
fi
if [ -n "$summary" ]; then
  gh pr comment "$pr_number" --body "$summary" >/dev/null || true
fi

url="$(gh pr view "$pr_number" --json url --jq '.url')"
echo "Ready for review: $url"
