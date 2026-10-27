# Architecture

How pdfv is put together, for whoever changes it next.

## Big picture

```
 keys ──► input::key_tokens ──► KeyState + Keymap ──┐
 mouse ─► input::mouse ─────────────────────────────┼──► Command ──► app::execute ──► Viewport / config / mode
 ':' ───► ui::cmdline ──► command::parse_ex ────────┤
 toolbar / status bar clicks ───────────────────────┘

 Viewport + Layout ──► render::tiles::plan ──► Renderer (worker threads, own MuPDF Document each)
                                                   │ finished tiles (egui ColorImage)
                                                   ▼
                             view::DocView: LRU texture cache ──► draw tiles
```

Every user action is a `Command` (src/input/command.rs), whichever input
produced it. Key bindings, `:` commands, mouse gestures and buttons all end
in `PdfvApp::execute`. That keeps bindings configurable and the logic
testable without a window.

## Threads

- **UI thread** (eframe): input, layout arithmetic, texture upload, drawing.
  It calls MuPDF only to read page sizes when a document is opened.
- **Render workers** (`render/mod.rs`, 2–4 threads): each opens its own MuPDF
  `Document` (MuPDF contexts aren't shared between threads) and renders tiles
  from a per-worker cache of display lists, so a page's content is interpreted
  once for all of its tiles. Results go back over a crossbeam channel, already
  converted to `egui::ColorImage`, and the worker wakes the UI.
- **Link loader** (`links.rs`): reads every page's links once at open time.

The render queue is a priority list the UI replaces wholesale whenever the
wanted set changes (`Renderer::set_queue`), so jobs for pages that scrolled
away are dropped before they start. Jobs already running aren't queued twice.

## Coordinates

- **Document space**, PDF points: pages stacked vertically by `layout.rs`
  with a gap, centred on the widest page.
- **View space**, screen points from the top-left of the page area:
  `viewport.rs` holds the scroll offset (document point at the view's
  top-left) and the zoom (screen points per PDF point). Pure arithmetic.
- **Device pixels**: render scale = zoom × `pixels_per_point` (2 on Retina),
  rounded to hundredths (`ScaleKey`) so nearly equal scales share the cache.
- **Page space**: link rectangles and destinations are stored relative to each
  page's top-left corner (MuPDF page bounds don't always start at 0,0).

## Rendering

Pages are split into 1024×1024 device-pixel tiles (`render/tiles.rs`).
`tiles::plan` lists, in priority order: visible tiles, a half-screen margin
to the sides, 3 screens ahead in the scroll direction, ¾ screen behind.
Already cached or failed tiles are filtered out.

Drawing (`view.rs`) fills each visible page with the page colour, then — if
any sharp tile is missing — draws the nearest cached scale stretched
underneath, then the sharp tiles 1:1 on the physical pixel grid (the page
corner is snapped to a pixel, so tiles neither blur nor seam). A page with no
tiles at all shows its page number.

After a zoom or resize, new renders wait 150 ms for the scale to settle; the
stretched old tiles cover the gap. The texture cache is an LRU bounded at
512 MB.

## Input

- `input/mod.rs` turns egui events into vim tokens: printable characters from
  text events (so they follow the keyboard layout), everything chorded or
  special from key events (`<C-d>`, `<S-Space>`).
- `input/keymap.rs` parses binding notation, holds bindings + prefixes, and
  `KeyState` accumulates counts and partial sequences. A sequence that is both
  bound and a prefix of a longer one waits for the next key.
- Keys typed in the same frame as `:` are fed to the command line by
  `ui::cmdline::type_ahead` (Enter submits, Esc cancels), so fast typing like
  `:12⏎` never loses characters.

## Config

`config.rs`: every option goes through `Config::set(key, toml::Value)`, used
both for the file and for `:set`, so they accept the same values. Errors are
collected as warnings, never fatal. Key overrides are applied on top of
`Keymap::default_normal()`.

## Modules

| File | Role |
| --- | --- |
| `main.rs` | CLI, logging, config load, window options |
| `app.rs` | `PdfvApp`: frame loop, command execution, screen split |
| `view.rs` | `DocView`: one open document (layout, renderer, textures, links) |
| `document.rs` | MuPDF wrapper: page sizes, links, `PageRenderer` |
| `layout.rs` | Page rectangles in document space |
| `viewport.rs` | Scroll/zoom/fit arithmetic, page navigation |
| `render/` | Worker pool + queue, tile planning, LRU cache |
| `input/` | Commands, key notation, counts, mouse |
| `links.rs` | Link types, loader thread, safe external opening |
| `config.rs` | Options, TOML loading, `:set` |
| `modes.rs` | Input modes |
| `ui/` | Status bar, command line, toolbar, help overlay, font lookup |

## Testing

`cargo test` runs the unit tests (layout, viewport, keymap, counts, commands,
config, tiles, queue, cache, history…). Tests that need a real PDF are
`#[ignore]`d and read the path from `PDFV_TEST_PDF`; one of them checks that a
rendered tile is pixel-identical to the same region of a whole-page render.
