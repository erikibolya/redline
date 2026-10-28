# UI — editor shell (v0.3)

Status: in progress. Replaces the v0.2 single-page canvas and session list.

## Goal

Make redline feel like a drawing tool rather than a form: sessions on the left, image in the
middle, properties on the right, and the agent-facing prose out of the way until you submit.

Reference points: Photoshop / Illustrator / Figma shell — persistent left navigator, floating tool
options, right inspector, status bar. We borrow the layout, not the feature set.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ header:   redline · <session title>            [status]      [Submit review] │
├──────────────┬───────────────────────────────────────────────┬───────────────┤
│ left rail    │  options bar (floating, top-centre)           │ right panel   │
│ 264px        │  [V][R][A][P][T]   colour   width   zoom      │ 300px         │
│ ┌──────────┐ │                                               │ ┌───────────┐ │
│ │ + New    │ │                                               │ │Annotns (3)│ │
│ │ search   │ │              image (zoom / pan)               │ │ 1 ▭ …     │ │
│ │ chips    │ │                                               │ │ 2 ↗ …     │ │
│ │          │ │                                               │ │ 3 T …     │ │
│ │ session  │ │                                               │ └───────────┘ │
│ │ session  │ │                                               │ ▸ Message to  │
│ │ session  │ │                                               │   agent       │
│ └──────────┘ │                                               │               │
├──────────────┴───────────────────────────────────────────────┴───────────────┤
│ status bar:  zoom 87% · fit · 100%      1440×900      482, 260      id       │
└──────────────────────────────────────────────────────────────────────────────┘
```

Columns collapse: `[` toggles the left rail, `]` the right panel. Both states persist in
`localStorage`. The canvas always fills what is left — no fixed page width, no centred 1000 px
column.

## Left rail — sessions as folders

- **+ New review** at the top (drop a screenshot anywhere in the window also works).
- Search field (title, note, id).
- Filter chips with counts: `All 7` · `Open 5` · `Submitted 2` · `Mine` (created by a human).
- Each row: agent-provided **title**, note excerpt, relative time (`2h ago`), status dot, and a
  badge: annotation count for submitted sessions, a **new** dot for anything created or submitted
  since this browser last looked (tracked per session in `localStorage`).
- Selected row is highlighted; clicking loads that session into the canvas without a page reload.
- Groups by day (`Today`, `Yesterday`, older) when more than ~15 sessions.

## Middle — canvas

- Renders at viewport size with a transform (crisp at every zoom, unlike CSS scaling a
  fixed-resolution canvas).
- **Zoom**: wheel with Ctrl/Cmd at the pointer, `+`/`-`, `0` fit, `1` 100%, double-click a tool
  button to reset. Range 10 %–800 %. Fit-to-window on load.
- **Pan**: space+drag, middle-drag, or plain wheel (shift for horizontal). Cursor changes.
- Image is centred; the stage has a dark checkerboard so transparent PNGs read correctly.

## Tools and interaction

| Key | Tool | Notes |
|-----|------|-------|
| `V` | Select | click to select, drag to move, `Delete` to remove, arrows nudge 1 px (shift 10) |
| `R` | Rectangle | drag; shift = square |
| `A` | Arrow | drag |
| `P` | Pen | freehand |
| `T` | Text | click, type inline at that point (no `prompt()`), `Enter` commits, `Esc` cancels |

- `Ctrl/Cmd+Z` undo, `Ctrl/Cmd+Shift+Z` redo (full history, not just "remove last").
- `Esc` cancels the in-progress shape or clears selection.
- `Ctrl/Cmd+Enter` submits the review.
- Hit testing covers rect edges+interior, arrow segment, pen polyline, text box.

## Right panel

- **Annotations** list: index, type glyph, text preview or coordinates; click selects (canvas
  highlights), bin icon deletes. Drag on canvas keeps this list in sync.
- **Message to agent** collapsed by default — this is the session `note` (what the agent asked).
  It is context, not the main event, so it hides behind a disclosure.
- Below it, a small `review.md` preview toggle (what the agent will read).

## Status bar

Zoom level, image dimensions, live cursor position in image pixels, session id. Cheap to add and
it makes the canvas feel instrumented.

## Server changes

- `GET /api/sessions` gains `annotation_count` per session (computed by reading each
  `annotations.json`) so the rail can show badges without N extra requests.
- UI assets split into `app.css`, `app.js`, `editor.js`, `list.js`, all inlined into the binary via
  `include_str!` and served from `/static/*`.

## Staging

1. **Shell** — 3 columns, zoom/pan, tools with a proper stage, status bar, right panel.
2. **Sessions rail** — filters, search, badges, switching sessions without reload; rebuilt list
   page using the same CSS/components.
3. **Editing depth** — undo/redo history, inline text editor, nudge, delete, drag-to-move.
4. **Polish** — light theme toggle, empty states, keyboard help overlay, focus rings.

Each stage is a tagged release; the old page keeps working until the new one covers it.

## Non-goals

Layers, blending modes, shapes beyond rect/arrow/pen/text, colour pickers beyond presets,
multi-select, exporting to other formats. This is a review tool, not an editor.
