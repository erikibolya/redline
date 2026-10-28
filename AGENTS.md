# Agent instructions

This repository is a local visual-review queue. The human annotates screenshots; you create
sessions and read the submitted JSON.

## Workflow

1. Ensure a session exists:
   `python3 review.py open <screenshot> --title "<component / state>" --url "<page>" --note "<what to check>" --json`
   (or `--sessions <dir>` if using a shared folder).
2. Open the printed `review_url` in a browser tab, or ask the human to open it.
3. Wait for the human:
   `python3 review.py wait <id> --timeout 600 --json`
4. Read `sessions/<id>/annotations.json` (coordinates + text), `annotated.png` (visual), and
   `review.md` (summary). Implement each note as a checklist item.
5. Re-screenshot and create a follow-up session for confirmation; remove obsolete sessions with
   `python3 review.py remove <id> --yes`.

## Rules

- Keep `sessions/` out of git; it is gitignored.
- Do not edit `annotations.json` by hand — the human's submission is the source of truth.
- Use the same screenshot dimensions as the live page; coordinates are canvas pixels.
- The server binds `127.0.0.1` by default; never expose it publicly without auth.
- Run `python3 -m unittest discover -s tests -t .` after changing server/CLI code.
