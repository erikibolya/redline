# Automatic feature integration

`dev` is the integration branch. It is protected: nobody pushes to it directly, and a feature only
lands after CI is green. The flow is deliberately gated on **ready for review**, not on every push,
so an agent can keep working on a draft branch without shipping half a feature.

## The flow

1. The agent works on a branch named `feat/…`, `fix/…`, `docs/…` or `chore/…` and pushes it.
2. `.github/workflows/draft-pr.yml` opens **one draft PR** into `dev` (if there is not one already).
   Draft means: do not merge yet.
3. CI (`desktop.yml`) runs on every PR now: Rust tests on Linux and Windows, browser e2e and the
   Tauri shell. These are the required checks on `dev`.
4. When the feature is done, the agent marks it ready:

   ```bash
   bash desktop/scripts/feature-ready.sh "one-line summary of the feature"
   ```

   The script refuses a dirty tree, pushes the branch, creates/finds the PR, marks it ready and
   comments the summary.
5. `.github/workflows/auto-merge.yml` sees `ready_for_review`, enables auto-merge and GitHub merges
   the PR once every required check is green. On failure the PR stays open; fix it and push again.
6. The merge is a push to `dev`, which starts `dev build`. The new `0.20.0-dev.N` appears in
   GitHub Releases; the desktop app installs it with **Install latest dev build…**.

## Rules

- **Draft while working, ready when done.** Opening a non-draft PR is also a ready signal; do not
  do it until the feature is actually finished.
- **Never push or merge into `dev` directly.** Branch protection rejects it, and the bot path is
  the one that runs the checks.
- **Do not force-push a branch after it is ready** unless you are prepared for the PR to update;
  auto-merge follows the branch head.
- The dev build is a rolling prerelease (tag `dev`); stable releases still come from `v*` tags on
  `main`. A dev build is for testing, not for production.

## Required checks

The `dev` branch requires all four jobs from `desktop.yml`:

- `core + cli (ubuntu-latest)`
- `core + cli (windows-latest)`
- `browser e2e`
- `tauri shell (windows)`

The E2E runs against a real Chromium and the server built from the PR, so a UI change that only
passes unit tests cannot land.

## Why not auto-merge every branch push?

Because a push is not a completion signal. Agents push checkpoints, WIP commits and fixes; merging
them would put unfinished features into the dev build and make the whole channel unreliable. The
draft PR plus `feature-ready.sh` is the explicit hand-off.

Verified end-to-end on 2026-10-29: draft PR -> feature-ready -> required CI -> auto-merge -> dev build.
