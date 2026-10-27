# PDF viewer — development plan

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Personal project, macOS (Apple Silicon) first; Linux support
later. Name: `vimpdf` (crate, binary, config and state paths; it was called
`pdfv` until 2026-10-27).

The target sits between zathura and okular: zathura's keyboard-first, minimal
UI, but with okular's mouse support and discoverability available when wanted.

This document is the brief for the coding agent. Work through the milestones in
order; each one must leave the app in a runnable, usable state.

## Status (2026-10-27)

All milestones M0–M11 are implemented and committed (one commit per
milestone, see `git log`). M9–M11 (annotations, redaction, menu bar) were
added on 2026-10-27 at the user's request. Each milestone section below has a **Status** note with what
was done, how its "done when" was checked, and any deviation from the plan.
User-facing documentation is in `README.md`, `docs/configuration.md` and
`config.example.toml`; code structure in `docs/architecture.md`.

Deviations from sections 2–3:

- **Not used:** `serde` (options are read one by one from `toml::Table`, which
  also gives per-entry warnings) and `arboard` (egui's clipboard is enough).
- **Added:** `winit` (to own the event loop, needed for M8) and, on macOS only,
  `objc2`, `objc2-foundation`, `objc2-app-kit` (the open-file handler).
- **Extra modules:** `view.rs` (one open document), `render/tiles.rs` (tiled
  rendering), `search.rs` doubles as the text worker, `jumplist.rs`,
  `state.rs`, `watch.rs`, `platform.rs`, `welcome.rs`, and `app.rs` is split
  into `app/hints.rs`, `app/searching.rs`, `app/visual.rs`, `app/reading.rs`.

Linux has not been built or tested (the README says so and lists the probable
build dependencies as a best guess). Not yet checked by a human: the visual appearance (bars, help, outline panel,
highlight colours, recolor), pinch-to-zoom on a real trackpad, and choosing
vimpdf inside Firefox's settings. Known limitations are listed per milestone
and in the README.

---

## 1. Goals and non-goals

**Goals**

- Open a PDF and read it entirely from the keyboard, with vim-like motions.
- Everything is also reachable with the mouse/trackpad: scrolling, zooming,
  following links, selecting and copying text, navigating the outline.
- Select and copy text from either input: mouse drag, or a vim-style visual mode.
- Smooth continuous scrolling, sharp rendering on Retina displays at any zoom.
- Never block the UI thread on rendering, text extraction or search.
- Fully configurable keybindings and appearance through a plain-text config file.
- Excellent for papers: fast search, link following, quick jump-and-return to
  references/figures/equations, remembered reading position.

**Non-goals (for now)**

- Writing a PDF parser or renderer — rendering is delegated to MuPDF.
- Editing page content, form filling, printing. (Annotations — highlights,
  comments — and redaction were added later as M9–M10.)
- CI, packaging, signing, distribution, Windows support.
- Multiple documents in tabs (one window = one document is fine).

---

## 2. Tech stack

| Concern | Choice | Notes |
| --- | --- | --- |
| Language | Rust, stable toolchain, edition 2021 or newer | |
| PDF rendering | `mupdf` crate (Rust bindings, builds MuPDF from source and links statically) | Requires Xcode Command Line Tools. AGPL-3.0 → the project is AGPL-3.0. Also used for structured text extraction (per-character positions) for selection. |
| Window + UI | `eframe` / `egui` | Pages drawn as textures; egui used for status bar, command line, toolbar, overlays, context menu. Keep rendering logic independent of egui so it could move to raw `winit` + `wgpu` later. |
| Config | `serde` + `toml` | |
| Paths | `directories` crate | Config and state locations. |
| Errors | `anyhow` in the binary, `thiserror` in library-style modules | |
| Logging | `tracing` + `tracing-subscriber` (env-filter) | `RUST_LOG=vimpdf=debug` |
| CLI args | `clap` (derive) | |
| File watching | `notify` (milestone 7) | |
| Channels | `crossbeam-channel` | Main thread ↔ render workers. |
| Clipboard | `arboard` (milestone 6) | Copying selected text. Use egui's clipboard output instead if it proves sufficient. |

**Important for the agent:** check the current versions and APIs of these crates
(docs.rs) before writing code — do not rely on remembered APIs, especially for
`mupdf` and `eframe`, which change between releases. Pin versions in `Cargo.toml`.

---

## 3. Architecture

```
src/
  main.rs          CLI parsing, logging, config loading, launch eframe
  app.rs           eframe::App impl: owns state, handles frame/update loop
  document.rs      Thin wrapper over mupdf: open, page count, page sizes,
                   render page region at scale, text search, links, outline,
                   structured text extraction
  render/
    mod.rs         Render worker pool + job queue + results channel
    cache.rs       LRU texture/pixmap cache keyed by (page, zoom bucket)
  layout.rs        Continuous vertical layout: page rects in document space
  viewport.rs      Scroll offset, zoom, fit mode, conversions doc <-> screen
  input/
    keymap.rs      Key-sequence parsing ("gg", "<C-d>", "<C-F>"), mode-aware bindings
    mouse.rs       Mouse/trackpad events -> Commands (wheel, pinch, click, drag,
                   double/triple click, right click), hit-testing via layout
    command.rs     enum Command { ScrollDown(n), GotoPage(n), ZoomIn, ... }
    counts.rs      Numeric count prefixes ("12G", "5j")
  modes.rs         Normal / Command (:) / Search (/) / Hints (f) / Outline /
                   Visual (v) / VisualLine (V)
  search.rs        Background search across pages, result list, n/N navigation
  links.rs         Link extraction, hint labels, jumplist (Ctrl-o / Ctrl-i)
  text.rs          Per-page text model as plain data: chars with quads, grouped
                   into words/lines/blocks in reading order; caret motions
                   (w/b/e/0/$/j/k), hit-testing a point -> char index
  selection.rs     Selection (anchor + caret, char- or line-wise), highlight
                   rects, extracting clean text (ligatures, de-hyphenation)
  config.rs        Config struct, defaults, TOML loading, keybinding overrides
  state.rs         Per-file persisted state (last page/zoom/position)
  ui/
    statusbar.rs   Bottom bar: file name, page x/y, zoom, mode, messages
    cmdline.rs     ':' and '/' input line with history
    toolbar.rs     Optional toolbar (hidden by default)
    help.rs        '?' overlay listing the active keybindings
    context_menu.rs Right-click menu
```

### Key design rules

1. **The UI thread never calls MuPDF.** It only issues jobs and uploads
   finished pixmaps as textures. Text extraction also runs on workers.
2. **MuPDF contexts are not shared across threads.** Each render worker opens its
   own `Document` from the file path. Page sizes, outline, links and per-page
   text models can be loaded on a worker and sent to the UI as plain data.
3. **Layout is computed once from page bounds** (in PDF points) at open time.
   Scrolling and zoom are pure arithmetic on that layout — no MuPDF calls.
4. **Render at physical pixel resolution**: scale = zoom × `pixels_per_point`
   (Retina = 2.0). Bucket zoom levels (e.g. round to 2 decimal places) so the
   cache gets hits.
5. **Visible pages first, then prefetch** one or two pages above/below. Cancel or
   deprioritise jobs for pages that have scrolled out of view.
6. **While a sharper render is pending, draw the stale texture scaled** (or a
   blank page-coloured rectangle with the page number). Never flash.
7. **All user actions go through `Command`.** Key handling maps keys → Commands;
   `:` commands parse into the same enum; mouse events, toolbar buttons and
   context-menu entries produce the same Commands too. This keeps bindings fully
   configurable and makes the logic testable without a window.
8. **The mouse is a first-class input, not an afterthought.** Every feature
   should have a sensible mouse path: pointer cursor over links, I-beam over
   text, drag selects text, right click opens a context menu, panels are
   clickable. Keyboard and mouse share the same state (e.g. a mouse selection
   can be extended with visual-mode motions, and vice versa). **Every mouse
   action must also have a keyboard equivalent** — the keyboard is never the
   second-class path. When adding a mouse interaction, add or point to its key.
9. **Pure logic gets unit tests**: keymap parsing, count prefixes, layout maths,
   viewport conversions, cache eviction, config parsing, caret motions,
   selection-to-text extraction.

---

## 4. Default bindings

Zathura/vim-like. All key bindings must be overridable in the config.

### Normal mode

| Key | Action |
| --- | --- |
| `j` / `k` | Scroll down / up a small step |
| `h` / `l` | Scroll left / right (when zoomed wider than window) |
| `<C-d>` / `<C-u>` | Half page down / up |
| `<C-f>` / `<C-b>`, `Space` / `<S-Space>` | Full screen down / up |
| `J` / `K` | Next / previous page (snap to page top) |
| `gg` / `G` | First / last page; `<n>G` goes to page n |
| `+` / `-` | Zoom in / out |
| `=` | Zoom to 100% |
| `s` | Fit width |
| `a` | Fit page |
| `/` | Search forward; `n` / `N` next / previous match |
| `f` | Link hints: show labels on visible links, type label to follow |
| `yf` | Link hints, but copy the chosen link's address instead of following it |
| `v` | Visual mode (caret at current search match if any, else first line in view) |
| `V` | Visual line mode (same anchor rules) |
| `gv` | Word hints: labels on visible words; typing one enters visual mode there |
| `<C-o>` / `<C-i>` | Jump back / forward (jumplist) |
| `m<letter>` / `'<letter>` | Set mark / jump to mark |
| `<Tab>` | Toggle outline (table of contents) |
| `<C-r>` | Toggle recolor (dark mode) |
| `<C-F>` | Toggle fullscreen |
| `<C-t>` | Toggle toolbar |
| `?` | Toggle help overlay (active keybindings) |
| `r` | Reload document |
| `:` | Command line |
| `Esc` | Cancel current mode / clear search highlight / clear selection |
| `q` | Quit |

### Visual / visual-line mode

The view auto-scrolls to keep the caret visible. Scroll keys (`<C-d>`, `<C-u>`,
`<C-f>`, `<C-b>`) still scroll and move the caret with the view.

| Key | Action |
| --- | --- |
| `h` / `l` | Caret left / right one character |
| `j` / `k` | Caret to next / previous line (reading order) |
| `w` / `b` / `e` | Next word start / previous word start / word end |
| `0` / `$` | Start / end of line |
| `{` / `}` | Previous / next text block (≈ paragraph) |
| `o` | Swap caret and anchor |
| `v` / `V` | Switch between char-wise and line-wise, or exit if already in that mode |
| `y` | Copy selection to clipboard and return to normal mode |
| `*` | Search the document for the selected text |
| `/`, `n`, `N` | Search; moves the caret to the match, extending the selection |
| `Esc` | Clear selection, back to normal mode |

Counts work as in normal mode (`3w`, `5j`).

### Mouse and trackpad

| Input | Action |
| --- | --- |
| Wheel / two-finger scroll | Scroll (vertical and horizontal) |
| Pinch, or `Cmd` + wheel | Zoom, anchored at the pointer |
| Click on a link | Follow it (pointer cursor on hover) |
| Drag on text | Select text (I-beam cursor over text) |
| Double click / triple click | Select word / line |
| `Shift` + click | Extend the current selection |
| `Alt` + drag, or middle drag | Pan the view |
| Right click | Context menu: copy (`y`), search for selection (`*`), open link (`f`) / copy link address (`yf`) |
| Click the page number in the status bar | Open `:` prefilled for go-to-page |
| Click an outline entry | Jump to it |

Command line: `:q`, `:open <path>`, `:<n>` (go to page n), `:set <option> <value>`,
`:source` (reload config), `:help`.

---

## 5. Config

Location: `~/.config/vimpdf/config.toml` (use this path on macOS too, rather than
`~/Library/Application Support`, so it lives next to other dotfiles).

```toml
[ui]
font_family = "Menlo"
font_size = 14
statusbar = true
toolbar = false
start_maximised = true

[colors]
page_background = "#ffffff"
recolor_dark = "#1e1e1e"
recolor_light = "#d4d4d4"
search_highlight = "#f9e2af80"
selection = "#89b4fa60"
caret = "#f38ba8"
hint_background = "#fab387"

[scroll]
step = 60            # points per j/k
smooth = true

[zoom]
step = 1.1
default_fit = "width"   # "width" | "page" | "none"

[mouse]
drag = "select"         # "select" | "pan"
pan_modifier = "alt"    # modifier that turns a drag into a pan
zoom_modifier = "cmd"   # modifier that turns the wheel into zoom
copy_on_select = false  # copy mouse selections to the clipboard immediately

[keys.normal]
"<C-F>" = "toggle_fullscreen"
"x" = "scroll_down 3"
# setting a key to "" unbinds it

[keys.visual]
"Y" = "yank_line"
```

Missing config file → defaults. Invalid entries → log a warning, show it in the
status bar, keep the default; never crash.

---

## 6. Milestones

Each milestone ends with: `cargo fmt`, `cargo clippy` clean, tests passing, a
manual check against a real paper, and a git commit.

### M0 — Skeleton

- `cargo new`, AGPL-3.0 `LICENSE`, `README.md` with build instructions.
- `vimpdf <file.pdf>` opens a window and shows page 1 rendered via MuPDF.
- Logging set up; clear error message for missing/invalid file.
- **Done when:** a real arXiv paper's first page displays correctly and sharply on Retina.
- **Status:** done (`de59782`). Page 1 rendered at Retina scale (2×) was checked from an image dump; the window was checked on a Retina display by the user.

### M1 — Continuous scrolling, motions, basic mouse

- Layout of all pages stacked vertically with a small gap, centred horizontally.
- Background render worker(s), job queue, result channel, LRU cache.
- `j`/`k`, `<C-d>`/`<C-u>`, `<C-f>`/`<C-b>`, `Space`, `J`/`K`, `gg`/`G`, counts (`5j`, `12G`).
- Trackpad/mouse wheel scrolling, vertical and horizontal.
- Mouse input routed through `input/mouse.rs` → `Command`, with hit-testing from screen to (page, PDF point).
- Links extracted per page on a worker; pointer cursor on hover; click follows the link (internal: go to the target page; external: `open` in the default browser). Exact-position destinations come in M5.
- **Done when:** scrolling through a 50-page paper at full speed never stutters or shows a blank page for more than a frame or two, memory stays bounded, and clicking a link in the paper goes to the right page.
- **Status:** done (`abfc7f7`). Holding `j` never showed a blank page; at an extreme 50 screens/s with `<C-f>`, figure-heavy pages (~190 ms to render) can stay blank for up to 3 frames. Texture cache bounded at 512 MB.

### M2 — Zoom and fit modes

- `+`/`-`/`=`/`s`/`a`, zoom anchored so the content under the viewport centre stays put.
- Pinch-to-zoom and `Cmd`+wheel, anchored at the pointer.
- Re-render at new scale with a short debounce; draw the scaled old texture meanwhile.
- Horizontal scrolling when the page is wider than the window.
- Fit mode re-applied on window resize.
- **Done when:** zooming in on an equation, by keyboard or by pinching, stays sharp and responsive.
- **Status:** done (`32d8343`). Deviation: pages are rendered in 1024 px tiles from per-worker MuPDF display lists, so high zoom only renders what's near the view. Rapid zooming renders only the final scale. Pinch was not injectable in tests (⌘+wheel was).

### M3 — Status bar, command line, config, discoverability

- Status bar: file name, `page/total`, zoom %, current mode, pending count/key sequence, transient messages. Clicking the page number opens `:` for go-to-page.
- `:` command line with the commands listed in section 4 and command history.
- Config loading (section 5), including keybinding overrides, `[mouse]` options and `:source`.
- Optional toolbar (hidden by default; `<C-t>` / `:set toolbar true`): zoom in/out, fit width/page, page number entry, search, outline. Buttons issue `Command`s.
- `?` / `:help` overlay listing the currently active bindings (generated from the keymap, so it reflects config overrides).
- Start maximised (sized to the screen's visible frame) when `start_maximised = true`, and make sure the window takes focus on launch.
- **Done when:** a custom binding in the config works after `:source` without restarting, and shows up in the `?` overlay.
- **Status:** done (`45818e9`). Fixed during testing: keys typed in the same frame as `:` were lost (now carried into the line). Later fixed in M7: egui's Tab focus could leave the status-bar page number focused, swallowing keys.

### M4 — Search

- `/` opens search input; search runs on a background thread page by page, starting from the current page and wrapping.
- Highlight all matches on visible pages, current match more strongly; `n`/`N` move between matches and scroll them into view.
- Status bar shows `match i/N` and "searching…" while in progress.
- Case-insensitive by default; smart-case if the query contains capitals.
- **Done when:** searching a word in a 100-page document shows the first hit immediately and finishes without freezing the UI.
- **Status:** done (`3153204`). First full search of a 117-page document ~550 ms in the background, repeats ~1 ms (page text is cached). Matches span line breaks and hyphenated words.

### M5 — Links and jumplist

- `f`: overlay short labels (home-row letters) on visible links; typing a label follows it; `Esc` cancels.
- `yf`: same hints, but copy the link's address (URL, or page reference for internal links) to the clipboard.
- Internal links (keyboard and mouse) jump to the exact destination position, not just the page.
- Jumplist: every non-local jump (link, `gg`, `G`, `:<n>`, search jump, mark) pushes the previous position; `<C-o>`/`<C-i>` move through it.
- Marks: `m<letter>` / `'<letter>`.
- **Done when:** in a paper, following a citation with `f` (or a click) and returning with `<C-o>` lands exactly where reading left off.
- **Status:** done (`076ed7f`). `<C-o>` after following a citation returns to the exact previous offset. `''` jumps back to the last jump origin. `yf` uses egui's clipboard.

### M6 — Text selection and visual mode

- Structured text extraction per page on a worker (MuPDF stext: chars with quads, lines, blocks), converted into the plain-data model in `text.rs` and cached alongside renders. Load lazily for visible pages.
- Mouse selection: drag selects (I-beam cursor over text), double click selects a word, triple click a line, `Shift`+click extends. `Alt`+drag / middle drag pans. Selection can span pages.
- Right-click context menu: copy, search for selection, open link / copy link address (when over a link).
- Visual modes `v` / `V` with the motions in section 4; caret drawn as a thin bar; view follows the caret. `*` searches for the selected text.
- Caret anchoring: current search match if any, else first line in view; `gv` shows word hints to place the caret anywhere visible.
- A mouse selection can be continued from the keyboard (enter visual mode with that selection), and a keyboard selection can be extended with `Shift`+click.
- `y` / `Cmd+C` copies to the clipboard. Clean the text on copy: expand ligatures (ﬁ → fi), join words hyphenated across line breaks, join wrapped lines within a block with spaces, keep block breaks as newlines.
- Known limitations to document in the README rather than solve: reading order in multi-column layouts follows MuPDF's block order and can occasionally jump; math typically copies poorly; scanned PDFs without a text layer have nothing to select.
- **Done when:** a paragraph from a two-column arXiv paper can be copied cleanly both with the mouse and using only the keyboard (e.g. `/` to the first words, `v`, `}` or `j`s, `y`).
- **Status:** done (`ed34d1b`). Tested on a two-column PRL paper: `v}y` copies a paragraph that continues into the next column cleanly; drag, double/triple click, Shift+click and the context menu work. Deviation: MuPDF's blocks are not paragraphs, so `PageText::from_raw` repairs the structure (merges line fragments, joins paragraphs across blocks and column breaks, splits at indents, drops margin line numbers of draft copies). `}`/`{` go to the end/start of the paragraph. Maths still copies poorly (font encodings).

### M7 — Reading comfort

- Recolor mode (`<C-r>`): render with inverted/adjusted colours for dark reading, configurable colours.
- Outline/TOC panel (`<Tab>`), navigable with `j`/`k`/`Enter` and clickable with the mouse.
- Persist per-file state (last position, zoom, fit mode) in `~/.local/state/vimpdf/` keyed by a hash of the canonical path; restore on open.
- Auto-reload when the file changes on disk (useful with LaTeX), keeping the current position.
- **Done when:** reopening a paper returns to the same spot; recompiling a LaTeX document updates the view in place.
- **Status:** done (`a8742d8`). Reopening restored page, zoom and marks; a real `pdflatex` recompile reloaded once and kept the position. State is also saved every 10 s and on `:open`. Added `ui.recolor` and `ui.auto_reload` options.

### M8 — macOS integration

- Build a proper `.app` bundle (e.g. with `cargo-bundle` or a small script) with an `Info.plist` declaring PDF as a supported document type, plus an icon.
- Handle files opened via Finder / Firefox "open with" (the macOS open-document Apple Event). Investigate what `winit`/`eframe` currently expose for this; if it isn't supported, document the gap and fall back to an AppleScript wrapper app that calls the binary with the file path.
- **Done when:** the app can be set as the PDF handler in Firefox and opens papers focused and maximised.
- **Status:** done (`64f2afd`, `dfbb2a8`). `scripts/bundle-macos.sh [--install]` builds `target/vimpdf.app`. winit 0.30 registers its own app delegate (its docs say otherwise) and aborts if it's replaced, so `application:openURLs:` is added to winit's delegate class at runtime; no AppleScript wrapper needed. Tested with `open -a`: launch with a file, a second file to the running app, launch without a file (welcome page). A newly opened file replaces the current one (one window).

### M9 — Highlights and comments

Annotations are standard PDF annotations, stored inside the PDF, so other
viewers (Preview, Acrobat, Zotero) show them too.

- An editor thread owns a writable copy of the document (`mupdf::pdf`). Every
  edit is applied there and written to a private working copy; render workers
  switch to it, and only the tiles of changed pages are re-rendered (per-page
  generation in the render key; old tiles stay as the fallback, so no flash).
- Highlight the visual selection (keyboard or mouse) in a colour: `a<letter>`
  in visual mode, with a configurable palette of colours and letters
  (default yellow `y`, green `g`, blue `b`, pink `p`, red `r`, orange `o`);
  `aa` uses the last colour.
- Comments: `c` in visual mode highlights and opens a comment editor (multi-line;
  Enter saves, Shift+Enter starts a new line, `Esc` cancels — modifier
  shortcuts don't reliably reach a focused text field on macOS, and Cmd+Enter
  is often taken by global shortcuts). Highlights with a comment get a
  small marker; hovering shows the comment.
- Annotations panel (`<C-a>` / `:annotations`): every highlight, comment and
  redaction mark in the document with page, colour, text and comment; `j`/`k`,
  Enter jumps, `e` edits the comment, `c<letter>` recolours, `d` deletes.
- Right-click on an annotation: edit comment, recolour, delete (each with its key).
- `u` undoes the last annotation change.
- Saving: `:w` writes the annotations into the PDF (incremental save, so the
  original bytes are kept), `:w <path>` saves a copy, `:q` refuses to quit
  with unsaved changes (`:q!` forces, `:wq` saves and quits); closing the window
  warns once. The status bar shows `[+]` while there are unsaved changes.
  vimpdf's own saves don't trigger auto-reload; an external change while there
  are unsaved changes is reported instead of reloaded.
- **Done when:** highlights in three colours and a comment added in vimpdf
  (keyboard only, and with the mouse) are saved with `:w`, survive reopening,
  and the PDF's annotations are listed with the right colours and comment by
  MuPDF on re-read.
- **Status:** done. Keyboard: three highlights in three colours and a two-line comment, `:w` (incremental: the original bytes are unchanged), `q` refused before saving; re-read by MuPDF with the right colours and comment; reopening loads them. Mouse: drag + right-click → Comment…, right-click → Delete, `u` undo. Fixed along the way: macOS replays file events from before a file is opened, so auto-reload now compares the file's size and time.

### M10 — Redaction (censoring)

Redaction permanently removes content; the result is always written to a new
file, never over the original.

- Mark text for redaction: `x` in visual mode (keyboard or mouse selection);
  `:redact-matches` marks every match of the current search (e.g. a name).
- Mark an area (figures, images): `X` enters area mode — a rectangle moved with
  `h`/`j`/`k`/`l`, resized with `H`/`J`/`K`/`L` (counts work), `Enter` marks it;
  or drag a rectangle with the mouse in that mode.
- Marks are redaction annotations (red outlines), listed in the annotations
  panel, removable and undoable like other annotations.
- `:redact [path]` (File → Apply redactions…) applies all marks: text under
  them is removed, image pixels are blanked, covered vector art is removed,
  black boxes are drawn; the file is fully rewritten with garbage collection
  (not incremental, so nothing removed survives in the file) to
  `<name>-redacted.pdf` unless a path is given, and then opened.
- After writing, the new file is re-read and the text inside every redacted
  area is checked to be gone; the result is reported.
- **Done when:** a name searched and redacted in a real paper, plus an area over
  a figure, are gone from the written file (no text extraction or search finds
  them, the figure pixels are blank), and the original file is unchanged.
- **Status:** done. On a copy of the LIGO paper: `/Hanford` + `:redact-matches` (4 matches on 3 pages) and an `X` area over part of Fig. 1, then `:redact`: the written file has no "Hanford" left (search finds 0, the re-read check passed), the area is black, the original is byte-identical. Finding: a label *inside* the figure ("Hanford, Wash." drawn as shapes) is not text, so search can't find it; documented — such content needs area marks. Metadata isn't stripped (documented).

### M11 — Menu bar

- A File / Edit / View menu bar at the top, shown by default (`ui.menubar`,
  `:set nomenubar` hides it). The button toolbar stays, below it, hidden by
  default.
- The menus are fully keyboard-driven: `gm` (command `menu`, rebindable; also
  `<F10>`, which macOS usually keeps for itself) opens File (also when the bar
  is hidden); `h`/`l` or ←/→ switch menus,
  `j`/`k` or ↑/↓ move, Enter or `l` opens a submenu / runs the entry, `h`
  leaves a submenu, `Esc` closes; each entry has an underlined letter that runs
  it directly. The mouse works as usual.
- Every entry runs a `Command` and shows its current key binding (looked up in
  the keymap, so config overrides are reflected).
- File: Open…, Open recent, Reload, Save, Save as…, Apply redactions…,
  Document properties, Quit. Edit: Undo, Copy, Highlight (colour submenu),
  Comment, Mark for redaction, Mark area for redaction, Annotations…, Find…,
  Find next/previous, Edit config file, Reload config. View: zoom in/out/100%,
  fit width/page, recolor, outline, annotations, toolbar, status bar, menu
  bar, fullscreen, keys (help).
- Open recent: the most recently opened files, from the saved state.
- Document properties: title, author, page count, file size, PDF version,
  number of annotations.
- **Done when:** every menu entry works from the keyboard (menu navigation and
  letters) and with the mouse, and shows the right key, including after
  rebinding a key in the config.
- **Status:** done. Keyboard: `gm ll p` (View → Fit page), `gm llw`, `gm i` (Document info), `gm llr` (recolor), Edit → Highlight → colour submenu, File → Open recent submenu, Esc; mouse: View → Zoom in. A test checks that rebinding `toggle_recolor` to `R` changes the key shown. Deviation: the default key is `gm` — `<F10>` stays bound, but macOS usually keeps the F-keys for itself (Mission Control, media keys).

### Later ideas (not scheduled)

- Faster first render of very heavy pages (e.g. a low-resolution preview), so
  even extreme-speed scrolling never shows a blank page.
- Opening several documents (tabs or windows) instead of replacing the open one.
- Sioyek-style reference preview: on hovering or hinting an internal link, show a popup of the destination region without jumping.
- Two-page / side-by-side mode.
- Rectangle/area selection (copy text inside a box, or copy the region as an image — handy for figures and equations).
- SyncTeX forward/backward search.
- Linux support (should mostly work already; test on Wayland and X11).

---

## 7. Working conventions for the agent

- Work one milestone at a time; don't start the next until the current one meets its "done when".
- Prefer small, focused modules and plain data types between threads.
- Don't add dependencies beyond section 2 without a short justification in the commit message.
- Ask the user to provide or point to a real PDF for testing (an arXiv paper is ideal); don't commit PDFs to the repo.
- When a library API is unclear, read its docs/source rather than guessing.
- Keep the README up to date with build steps, keybindings, mouse actions and config options.
- The user's shell is fish; any shell commands given to the user should be fish-compatible.
