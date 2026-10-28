# Contributing

## Setup

```bash
git clone <personal-repo-url> au-visual-review
cd au-visual-review
python3 review.py serve --open
```

No dependencies or build step. Python 3.10+.

## Development loop

```bash
python3 -m unittest discover -s tests -t . -v
python3 review.py serve --port 8790 --sessions /tmp/review-sessions
```

Use a throwaway sessions directory while developing so your local review queue stays clean.

## Code style

- Python stdlib only (no `requests`, no Flask); keep `review.py` portable.
- Keep the HTML/CSS/JS dependency-free and readable; no bundler.
- 4 spaces in Python; small functions; docstrings for public CLI/API behavior.
- Keep the JSON schema additive. If a field must change, document it in `docs/API.md` and `PLAN.md`.
- Treat `sessions/` as user data: never commit or rewrite it outside normal submission code.

## Tests

- Add a test for every new endpoint/CLI action or schema change.
- HTTP tests should use an ephemeral port and a temporary sessions directory.
- Run the full suite before committing.

## Commit / release

- One logical change per commit; describe the behavior, not the file list.
- Update `README.md`, `docs/API.md`, `docs/ARCHITECTURE.md` and `PLAN.md` together with behavior.
- Tag releases `v0.x.y`; no package publication is required for the current workflow.

## Personal GitHub

This repo is intentionally standalone:

```bash
git remote add origin git@github.com:<you>/au-visual-review.git
git push -u origin main
```

Do not add company remotes, tokens or internal URLs.
