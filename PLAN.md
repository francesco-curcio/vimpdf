# PDF viewer — development plan

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Personal project, macOS (Apple Silicon) first; Linux support
later. Working name: `pdfv` (rename freely — only the crate name and config
paths depend on it).

The target sits between zathura and okular: zathura's keyboard-first, minimal
UI, but with okular's mouse support and discoverability available when wanted.

This document is the brief for the coding agent. Work through the milestones in
order; each one must leave the app in a runnable, usable state.

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
- Annotation/editing, form filling, printing.
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
| Logging | `tracing` + `tracing-subscriber` (env-filter) | `RUST_LOG=pdfv=debug` |
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

Location: `~/.config/pdfv/config.toml` (use this path on macOS too, rather than
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
- `pdfv <file.pdf>` opens a window and shows page 1 rendered via MuPDF.
- Logging set up; clear error message for missing/invalid file.
- **Done when:** a real arXiv paper's first page displays correctly and sharply on Retina.

### M1 — Continuous scrolling, motions, basic mouse

- Layout of all pages stacked vertically with a small gap, centred horizontally.
- Background render worker(s), job queue, result channel, LRU cache.
- `j`/`k`, `<C-d>`/`<C-u>`, `<C-f>`/`<C-b>`, `Space`, `J`/`K`, `gg`/`G`, counts (`5j`, `12G`).
- Trackpad/mouse wheel scrolling, vertical and horizontal.
- Mouse input routed through `input/mouse.rs` → `Command`, with hit-testing from screen to (page, PDF point).
- Links extracted per page on a worker; pointer cursor on hover; click follows the link (internal: go to the target page; external: `open` in the default browser). Exact-position destinations come in M5.
- **Done when:** scrolling through a 50-page paper at full speed never stutters or shows a blank page for more than a frame or two, memory stays bounded, and clicking a link in the paper goes to the right page.

### M2 — Zoom and fit modes

- `+`/`-`/`=`/`s`/`a`, zoom anchored so the content under the viewport centre stays put.
- Pinch-to-zoom and `Cmd`+wheel, anchored at the pointer.
- Re-render at new scale with a short debounce; draw the scaled old texture meanwhile.
- Horizontal scrolling when the page is wider than the window.
- Fit mode re-applied on window resize.
- **Done when:** zooming in on an equation, by keyboard or by pinching, stays sharp and responsive.

### M3 — Status bar, command line, config, discoverability

- Status bar: file name, `page/total`, zoom %, current mode, pending count/key sequence, transient messages. Clicking the page number opens `:` for go-to-page.
- `:` command line with the commands listed in section 4 and command history.
- Config loading (section 5), including keybinding overrides, `[mouse]` options and `:source`.
- Optional toolbar (hidden by default; `<C-t>` / `:set toolbar true`): zoom in/out, fit width/page, page number entry, search, outline. Buttons issue `Command`s.
- `?` / `:help` overlay listing the currently active bindings (generated from the keymap, so it reflects config overrides).
- Start maximised (sized to the screen's visible frame) when `start_maximised = true`, and make sure the window takes focus on launch.
- **Done when:** a custom binding in the config works after `:source` without restarting, and shows up in the `?` overlay.

### M4 — Search

- `/` opens search input; search runs on a background thread page by page, starting from the current page and wrapping.
- Highlight all matches on visible pages, current match more strongly; `n`/`N` move between matches and scroll them into view.
- Status bar shows `match i/N` and "searching…" while in progress.
- Case-insensitive by default; smart-case if the query contains capitals.
- **Done when:** searching a word in a 100-page document shows the first hit immediately and finishes without freezing the UI.

### M5 — Links and jumplist

- `f`: overlay short labels (home-row letters) on visible links; typing a label follows it; `Esc` cancels.
- `yf`: same hints, but copy the link's address (URL, or page reference for internal links) to the clipboard.
- Internal links (keyboard and mouse) jump to the exact destination position, not just the page.
- Jumplist: every non-local jump (link, `gg`, `G`, `:<n>`, search jump, mark) pushes the previous position; `<C-o>`/`<C-i>` move through it.
- Marks: `m<letter>` / `'<letter>`.
- **Done when:** in a paper, following a citation with `f` (or a click) and returning with `<C-o>` lands exactly where reading left off.

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

### M7 — Reading comfort

- Recolor mode (`<C-r>`): render with inverted/adjusted colours for dark reading, configurable colours.
- Outline/TOC panel (`<Tab>`), navigable with `j`/`k`/`Enter` and clickable with the mouse.
- Persist per-file state (last position, zoom, fit mode) in `~/.local/state/pdfv/` keyed by a hash of the canonical path; restore on open.
- Auto-reload when the file changes on disk (useful with LaTeX), keeping the current position.
- **Done when:** reopening a paper returns to the same spot; recompiling a LaTeX document updates the view in place.

### M8 — macOS integration

- Build a proper `.app` bundle (e.g. with `cargo-bundle` or a small script) with an `Info.plist` declaring PDF as a supported document type, plus an icon.
- Handle files opened via Finder / Firefox "open with" (the macOS open-document Apple Event). Investigate what `winit`/`eframe` currently expose for this; if it isn't supported, document the gap and fall back to an AppleScript wrapper app that calls the binary with the file path.
- **Done when:** the app can be set as the PDF handler in Firefox and opens papers focused and maximised.

### Later ideas (not scheduled)

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
