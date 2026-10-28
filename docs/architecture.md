# Architecture

How vimpdf is put together, for whoever changes it next.

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
in `VimpdfApp::execute`. That keeps bindings configurable and the logic
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
- **Search worker** (`search.rs`): its own `Document`; scans pages from the
  current one and wraps, streaming matches per page so the first hit can be
  shown at once. Page text is cached, so repeat searches take milliseconds.
  A newer request cancels the running one between pages.

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

egui's own keyboard focus is released every frame unless a text field is
being edited (`ctx.text_edit_focused()`), so Tab/Enter/Space never activate
an egui widget behind vimpdf's back; clickable labels use `Sense::CLICK`
(not focusable).

- `input/mod.rs` turns egui events into vim tokens: printable characters from
  text events (so they follow the keyboard layout), everything chorded or
  special from key events (`<C-d>`, `<S-Space>`).
- `input/keymap.rs` parses binding notation, holds bindings + prefixes, and
  `KeyState` accumulates counts and partial sequences. A sequence that is both
  bound and a prefix of a longer one waits for the next key.
- Keys typed in the same frame as `:` are fed to the command line by
  `ui::cmdline::type_ahead` (Enter submits, Esc cancels), so fast typing like
  `:12⏎` never loses characters.

## Text and search

`document::page_text` turns MuPDF's structured text into `text::PageText`:
characters with page-relative boxes, font size and baseline, grouped into
lines and blocks (≈ paragraphs). `PageText::from_raw` repairs MuPDF's raw
structure with a few heuristics based on font size and baselines (glyph boxes
vary too much between fonts): it drops margin line numbers, merges fragments
of one visual line (justified text, sub/superscripts) inserting spaces at
gaps, joins blocks that continue a paragraph (also across a column break) and
splits blocks into paragraphs at first-line indents and larger gaps. `PageText::find` searches a normalised copy (whitespace runs
collapsed, lines joined with a space, words hyphenated at a line end joined
back, optional case folding) and maps matches back to character indices;
`PageText::rects` gives one rectangle per line of a match. The UI keeps
results in `search::SearchState` (per document) for `n`/`N` and highlighting.

## Selection and visual mode

`selection.rs` works on the page texts the UI has loaded (`DocText`):
positions are (page, character index); motions (`w`, `b`, `e`, `j`/`k` with a
preferred x, `{`/`}` …) cross pages when the neighbouring page's text is
loaded. `Selection` (anchor + caret, char- or line-wise) produces highlight
spans per page and the cleaned-up text for copying. The text worker in
`search.rs` also serves page text on request; `DocView::request_text` asks
for the visible pages and their neighbours every frame (each page once).

`app/visual.rs` holds the app side: entering visual mode (at the current
search match or the first line on screen), caret motions and keeping the caret
visible, copying, mouse dragging / double / triple / Shift-click selection,
panning, and the right-click menu (whose entries all name their keys).
`app/hints.rs` handles both link hints and word hints (`gv`).

## Links, hints, jumps

Links are loaded once per document (`links.rs`) with page-relative rectangles
and, for internal links, the destination page and point. `f` collects the
links intersecting the view, sorts them in reading order and gives them
prefix-free labels (`links::hint_labels`); hint mode (`Mode::Hints`) consumes
keys until a label is complete. `jumplist.rs` keeps the jump history
(browser-like: a new jump drops the forward entries) and the marks, both as
page + offset positions. Every jump command records the position first.

## Reading comfort

- Recolor is part of the render job key (`render::Recolor`), so dark and
  normal tiles are cached separately; the worker maps each pixel's lightness
  onto the dark → light palette before upload.
- The outline is read with the links (`document::load_links_and_outline`),
  flattened with depths, and shown by `ui/outline.rs`; `app/reading.rs`
  handles its keys.
- `state.rs` stores per-file state as TOML under the state directory, named
  by an FNV-1a hash of the canonical path (stable across Rust versions). Each
  state also records the document's fingerprint (`document::fingerprint`: the
  first half of the PDF `/ID`, which survives edits; else a file hash) and how
  many annotations that copy holds, so `state::find_copies` can restore the
  place for a new copy of the same paper and point to an annotated one.
  `platform::is_temporary` detects browser-style temporary copies, where `:w`
  is refused in favour of `:saveas`.
- `watch.rs` watches the file's directory with `notify` (tools often replace
  files by renaming); the app reloads 300 ms after the last change.

## macOS integration

`main.rs` creates the winit event loop itself (`eframe::create_native` +
`run_app`) instead of `eframe::run_native`, to run code between the loop's
creation and its start. There `platform::install_open_handler` adds an
`application:openURLs:` method to winit's application delegate class with
`class_addMethod` (winit 0.30 registers its own delegate and aborts if it is
replaced). Opened paths are queued; a file opened while launching is already
queued when eframe creates the app, later ones are picked up each frame.
Without a document vimpdf opens `welcome.rs`'s generated one-page PDF.
`scripts/bundle-macos.sh` builds the `.app` with an `Info.plist` declaring
`com.adobe.pdf` as a viewable document type.

## Annotations

`annotate.rs` runs an editor thread with a writable `mupdf::pdf::PdfDocument`
of the open file. The UI sends `EditRequest`s (create a highlight/comment/
redaction mark/sticky note/text box, set contents, colour, rectangle or font
size, delete, undo, save); the editor applies
them, keeps an undo stack of inverse operations (with a remap of object numbers
for annotations recreated by undo), and after each change writes the whole
document to a new working-copy file in a temp directory. The UI then calls
`DocView::content_changed`: the render workers reopen the working copy and the
changed pages' *generation* is bumped. The generation is part of the tile key,
so old tiles of those pages remain the fallback until the new ones arrive (no
flash), and other pages' tiles stay valid. `:w` saves into the original file
incrementally (append-only); the file's size and time are remembered so the
watcher doesn't reload vimpdf's own save. Redaction marks are `Redact` annotations. Applying them
(`EditorState::apply_redactions`) saves the current state to a scratch file,
opens that as a separate document, runs MuPDF's page redaction with black
boxes, pixel blanking for images, removal of covered line art and text
removal, writes the result as a full rewrite with garbage collection, then
re-opens the written file and checks the text under every mark is gone.
Sticky notes are `Text` annotations and text boxes `FreeText` annotations
(Helvetica, the colour and size in the default appearance; MuPDF draws them).
A text box's height always follows its text: `text_box_height` mirrors
MuPDF's line breaking (1.2 line height, breaks at spaces), and the box is
refitted after every change to its text, size or width, so undo restores it.
`app/redact.rs` has marking (selection, search matches) and the result
handling. `app/place.rs` is placement mode (`Mode::Place`): a box moved and
resized with the keyboard or the mouse, which becomes a redaction area (`X`),
a note (`C`), a text box (`T`), or the new place of an existing one.
`app/annotations.rs` holds the UI side of annotations: selection → rectangles
per page, the comment editor (`ui/comment.rs`), the panel
(`ui/annotations.rs`), comment bubbles, hover outlines and tooltips, the
quit/reload guards. `app/visual.rs` turns a press on a bubble, note or text
box into an edit (click) or a move (drag), and builds the right-click menu.

## Menus

`ui/menubar.rs` draws the menu bar and its menus by hand (not with egui's
menu widgets) so they can be driven from the keyboard: `MenuState` holds the
open menu, the highlighted entry and an open submenu, and `MenuState::key`
implements vim-key navigation and underlined-letter shortcuts (unit-tested).
`app/menu.rs` builds the menus from the current state every frame (enabled
entries, check marks, recent files from the state directory, the highlight
palette) and labels each entry with its current binding via
`Keymap::key_for`, so config overrides show up.

## Config

`config.rs`: every option goes through `Config::set(key, toml::Value)`, used
both for the file and for `:set`, so they accept the same values. Errors are
collected as warnings, never fatal. Key overrides are applied on top of
`Keymap::default_normal()`.

## Modules

| File | Role |
| --- | --- |
| `main.rs` | CLI, logging, config load, window options |
| `app.rs` | `VimpdfApp`: frame loop, command execution, screen split |
| `view.rs` | `DocView`: one open document (layout, renderer, textures, links) |
| `document.rs` | MuPDF wrapper: page sizes, links, `PageRenderer` |
| `layout.rs` | Page rectangles in document space |
| `viewport.rs` | Scroll/zoom/fit arithmetic, page navigation |
| `render/` | Worker pool + queue, tile planning, LRU cache |
| `input/` | Commands, key notation, counts, mouse |
| `links.rs` | Link types, loader thread, safe external opening |
| `text.rs` | Page text model, matching, match rectangles |
| `search.rs` | Search worker and result state |
| `jumplist.rs` | Jump history and marks |
| `state.rs` | Per-file saved position, zoom, fit, marks |
| `watch.rs` | File change notifications |
| `app/reading.rs` | Recolor, outline keys, state save/restore, auto-reload |
| `ui/outline.rs` | Outline panel |
| `platform.rs` | Files opened from the system (macOS `application:openURLs:`), temporary-folder checks, the private temp dir |
| `present.rs` | Present mode: mailbox on Linux where the window's surface supports it (asked with a throwaway surface; the window starts on vsync and switches on its first frame), `VIMPDF_PRESENT_MODE` override |
| `annotate.rs` | Annotation editor thread: highlights, comments, redaction marks, undo, saving |
| `app/annotations.rs`, `ui/annotations.rs`, `ui/comment.rs` | Annotations UI |
| `app/redact.rs` | Redaction marks, applying |
| `app/place.rs` | Placement mode: redaction areas, notes, text boxes, moving |
| `ui/menubar.rs`, `app/menu.rs` | Menu bar: widget + keyboard navigation, contents |
| `welcome.rs` | The page shown when no document is open |
| `scripts/` | App bundle and icon generation (macOS), install with desktop entry (Linux) |
| `selection.rs` | Text positions, caret motions, selections, clean copying |
| `app/hints.rs`, `app/searching.rs`, `app/visual.rs` | App-side hint mode, search, visual mode and mouse selection |
| `config.rs` | Options, TOML loading, `:set` |
| `modes.rs` | Input modes |
| `ui/` | Status bar, command line, toolbar, help overlay, font lookup |

## Testing

`cargo test` runs the unit tests (layout, viewport, keymap, counts, commands,
config, tiles, queue, cache, history…). Tests that need a real PDF are
`#[ignore]`d and read the path from `VIMPDF_TEST_PDF`; one of them checks that a
rendered tile is pixel-identical to the same region of a whole-page render.
