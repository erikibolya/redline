# Automatic feature integration

`dev` is the integration branch. It is protected: nobody pushes to it directly, and a feature only
lands after CI is green. The flow is deliberately gated on **ready for review**, not on every push,
so an agent can keep working on a draft branch without shipping half a feature.

## The flow

1. The agent works on a branch named `feat/…`, `fix/…`, `docs/…` or `chore/…` and pushes it.
   The branch is visible on GitHub, but it does not create a PR or a build yet.
2. When the feature is done, the agent marks it ready:

   ```bash
   bash desktop/scripts/feature-ready.sh "one-line summary of the feature"
   ```

   The script refuses a dirty tree, pushes the branch, creates a ready PR into `dev` (or marks an
   existing draft PR ready) and comments the summary.
3. CI (`desktop.yml`) runs on every PR now: Rust tests on Linux and Windows, browser e2e and the
   Tauri shell. These are the required checks on `dev`.
4. `.github/workflows/auto-merge.yml` sees the ready PR (`opened` or `ready_for_review`), enables
   auto-merge and GitHub merges it once every required check is green. On failure the PR stays
   open; fix it and push again.
5. The merge is a push to `dev`, which starts `dev build`. The new `0.20.0-dev.N` appears in
   GitHub Releases; the desktop app installs it with **Install latest dev build…**.

## Rules

- **Ready means done.** The PR is created by `feature-ready` when the feature is finished. If
  you open a PR manually earlier, keep it a draft; auto-merge ignores drafts.
- **Never push or merge into `dev` directly.** Branch protection rejects it, and the bot path is
  the one that runs the checks.
- **Do not force-push a branch after it is ready** unless you are prepared for the PR to update;
  auto-merge follows the branch head.
- The dev build is a rolling prerelease (tag `dev`); stable releases still come from `v*` tags on
  `main`. A dev build is for testing, not for production.

## Required checks

Both `dev` and `main` require all four jobs from `desktop.yml`:

- `core + cli (ubuntu-latest)`
- `core + cli (windows-latest)`
- `browser e2e`
- `tauri shell (windows)`

The E2E runs against a real Chromium and the server built from the PR, so a UI change that only
passes unit tests cannot land.

## Releases

`main` is protected by the same rule: releases are a PR from `dev` to `main`, never a direct push.
Auto-merge is deliberately disabled for `main` (the workflow only targets `dev`); a human merges
the release PR after the checks and an explicit yes, then tags `vX.Y.Z`.

## Other repositories

The mechanism is not RedLine-specific:

- `feature-ready` is installed globally at `/mnt/user/appdata/agent-tools/bin/feature-ready`. It
  uses `dev` when it exists, otherwise the repository's default branch, creates/marks the PR ready
  and enables GitHub auto-merge when the target is protected with required checks.
- Each repository still needs its own branch protection: the check names differ because every
  project has different CI. Once the target is protected with at least one required check,
  `feature-ready` works without any repository-specific script or workflow.
- The `auto-merge.yml` GitHub workflow is optional for repositories where a human (rather than
  `feature-ready`) marks PRs ready. The command uses GitHub's native auto-merge directly, so it
  works in a repo that has no workflow file at all.

## Why not auto-merge every branch push?

Because a push is not a completion signal. Agents push checkpoints, WIP commits and fixes; merging
them would put unfinished features into the dev build and make the whole channel unreliable.
`feature-ready.sh` is the explicit hand-off; auto-merge only reacts to a ready PR.

Verified end-to-end on 2026-10-29: ready PR -> required CI -> auto-merge -> dev build.
