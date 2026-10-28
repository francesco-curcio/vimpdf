# vimpdf

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Everything works from the keyboard; the mouse works too
when you want it. Rendering is done by [MuPDF](https://mupdf.com/); the window
and UI use [egui/eframe](https://github.com/emilk/egui).

Status: in development — see [PLAN.md](PLAN.md) for the roadmap. Done so far:
continuous scrolling, vim motions with counts, links, zoom and fit modes,
status bar, `:` command line, config file, toolbar, help overlay, search,
link hints, jumplist and marks, text selection with vim visual mode and the
mouse, recolor (dark mode), outline panel, remembered positions, auto-reload,
a macOS app bundle and a Linux desktop entry that make it the default PDF
viewer (Finder, file managers, Firefox),
highlights, comments, sticky notes and text boxes saved into the PDF, and
redaction (censoring).

Most things are reachable three ways: a vim-style key, the `:` command line,
and the File / Edit / View menus (which show each entry's key, and are
themselves driven from the keyboard with `gm`).

Documentation:

- this README — building, running, keys, mouse, command line
- [docs/configuration.md](docs/configuration.md) — every config option
- [config.example.toml](config.example.toml) — a commented example config
- [docs/architecture.md](docs/architecture.md) — how the code is organised

## Building

Requirements on macOS:

- Xcode Command Line Tools (`xcode-select --install`), needed to compile MuPDF.
- Rust stable via [rustup](https://rustup.rs/). On Apple Silicon make sure the
  toolchain is native: `rustc -vV` should print `host: aarch64-apple-darwin`.
  An x86_64 toolchain builds an Intel binary that runs under Rosetta.

```fish
cargo build --release
```

### Linux

Builds, passes the tests and runs on Arch Linux (CachyOS), under Wayland
(KDE Plasma) and X11 (XWayland).
Building needs a C/C++ compiler, `make` and libclang (MuPDF is compiled from
source and its bindings are generated with bindgen):

```sh
sudo pacman -S base-devel clang                 # Arch
sudo apt install build-essential libclang-dev   # Debian/Ubuntu
sudo dnf install gcc gcc-c++ make clang-devel   # Fedora
```

Rust via [rustup](https://rustup.rs/), then `cargo build --release` as above.
At run time vimpdf loads the usual desktop libraries itself (Wayland or X11,
libxkbcommon, OpenGL, fontconfig); any desktop install has them.

To install it with a menu entry and icon, and register it as a PDF viewer
(so it appears in "Open With" and can be picked by browsers):

```sh
scripts/install-linux.sh             # ~/.local/bin/vimpdf + ~/.local/share/applications/vimpdf.desktop
scripts/install-linux.sh --default   # …and make it the default PDF viewer (xdg-mime)
```

Differences from macOS: `Ctrl` takes the place of `Cmd` (`Ctrl` + wheel
zooms, `Ctrl+C` copies); trackpad pinch-to-zoom isn't available (winit only
delivers pinch gestures on macOS), use `Ctrl` + wheel or `+`/`-`; and every
file opened from outside starts its own window rather than replacing the
document in a running one. Dropping a PDF on the window works under X11 but
not Wayland (winit doesn't support it there yet); use `:open`. In Firefox, choose vimpdf under Settings →
General → Applications → "Portable Document Format (PDF)" (after
`--default`, "Use system default" does it).

The first build compiles MuPDF from source and takes a minute or two.

To run it from anywhere as `vimpdf`:

```fish
cargo install --path .
fish_add_path ~/.cargo/bin   # once, if ~/.cargo/bin isn't on your PATH
```

### macOS app (Finder, Dock, Firefox)

```fish
scripts/bundle-macos.sh             # builds target/vimpdf.app
scripts/bundle-macos.sh --install   # …and copies it to ~/Applications
```

The bundle declares PDF as a document type it can view, so after installing
it:

- **Finder**: right-click a PDF → Open With → vimpdf. To make it the default,
  Get Info (⌘I) on a PDF → Open with: vimpdf → Change All….
- **Firefox**: Settings → General → Applications → "Portable Document Format
  (PDF)" → Use other… → choose `~/Applications/vimpdf.app`. Clicking a PDF
  link then downloads it and opens it in vimpdf.
- Opening a PDF while vimpdf is running replaces the open document (one window,
  one document). Dropping a PDF on the window does the same.
- Launched without a file (e.g. from the Dock), vimpdf shows a short welcome
  page; open a file with `:open <path>` or by dropping it on the window.

The app is signed ad hoc (no Apple developer certificate), which is fine for
a locally built app. `scripts/make-icon.sh` regenerates the icon.

## Usage

```fish
vimpdf paper.pdf
vimpdf --config other.toml paper.pdf   # use a different config file
vimpdf                                  # welcome page; :open a file from there
```

Key notation follows vim: `<C-d>` is Ctrl+d, `<C-F>` is Ctrl+Shift+f,
`<S-Space>` is Shift+Space. Counts work as in vim: `5j`, `3J`, `12G`, `3+`.
Press `?` in the viewer for the list of the bindings that are actually active
(including your config overrides).

### Keys

| Key | Action |
| --- | --- |
| `j` / `k`, `↓` / `↑` | Scroll down / up |
| `h` / `l`, `←` / `→` | Scroll left / right (when the page is wider than the window) |
| `<C-d>` / `<C-u>` | Half screen down / up |
| `<C-f>` / `<C-b>`, `Space` / `<S-Space>`, `PageDown` / `PageUp` | Full screen down / up |
| `J` / `K` | Next / previous page (snaps to the page top) |
| `gg` / `G`, `Home` / `End` | First page / end of document; `<n>G` or `<n>gg` goes to page n |
| `+` / `-` | Zoom in / out (one `zoom.step` each) |
| `=` | Zoom to 100% |
| `s` | Fit width (kept when the window is resized) |
| `a` | Fit page (kept when the window is resized; `J`/`K` keep pages centred) |
| `/` | Search (Enter runs it; an empty search repeats the last one) |
| `n` / `N` | Next / previous match (wraps; `3n` skips ahead) |
| `f` | Link hints: a label appears on every link on screen; type it to follow the link |
| `yf` | Link hints, but copy the link's address instead of following it |
| `<C-o>` / `<C-i>` | Back / forward through the jump history |
| `m<letter>` / `'<letter>` | Set a mark / jump to it (`` ` `` works like `'`) |
| `''` | Back to where the last jump started |
| `v` / `V` | Visual mode (select text) / visual-line mode — see below |
| `gv` | Word hints: a label on every word on screen; typing one starts visual mode there |
| `<C-a>` | Annotations panel (all highlights, comments and redaction marks) |
| `u` | Undo the last annotation change |
| `X` | Mark an area for redaction (see Redaction) |
| `C` | Add a sticky note (see Highlights, comments, notes and text) |
| `T` | Add a text box, written on the page |
| `D` | Delete an annotation: a label appears on each one on screen; type it |
| `:` | Command line |
| `gm` | Open the menu bar (File / Edit / View) — see Menus |
| `?` | Help overlay: active keys, mouse actions, commands. Your scroll keys (`j`/`k`, `<C-d>`/`<C-u>`, `gg`/`G`, counts…) scroll it; `?`, `Esc` or `q` close it |
| `<Tab>` | Outline (table of contents) panel |
| `<C-r>` | Toggle recolor (dark mode) |
| `<C-t>` | Toggle the toolbar |
| `<C-F>` | Toggle fullscreen |
| `r` | Reload the document, keeping the position |
| `Esc` | Cancel a pending count/key sequence, clear the message and search highlights, close help |
| `q` | Quit |

### Mouse and trackpad

Every mouse action also has a key.

| Mouse | Action | Key equivalent |
| --- | --- | --- |
| Wheel / two-finger scroll | Scroll | `j` `k` `h` `l` |
| Pinch, or `Cmd` + wheel (`Ctrl` + wheel on Linux) | Zoom, anchored at the pointer | `+` `-` |
| Click a link | Follow it | `f` + label |
| Drag over text | Select it | `v` + motions |
| Double / triple click | Select a word / a line | `gv` + label, `V` |
| `Shift` + click | Extend the selection | motions in visual mode |
| `Alt` + drag, middle drag | Pan | `h` `j` `k` `l` |
| Right click | Menu: copy, search for selection, highlight, comment, open link, copy link address; on an annotation: edit comment, change colour, delete | `y`, `*`, `a<letter>`, `c`, `f`, `yf`; `<C-a>` then `e`, `c<letter>`, `d` |
| Click the page number in the status bar | Go to page | `:<n>` |
| Click an outline entry | Jump to it (panel stays open) | `<Tab>`, `j`/`k`, Enter |
| Toolbar buttons | Zoom, fit, go to page, help | `+` `-` `s` `a` `:<n>` `?` |

Internal links jump to the exact destination (a citation lands on the
reference, with a little context above it). External links open in the
default browser — only `http(s)` and `mailto` links, so a PDF can't use a link
to launch local files or applications.

### Jumps and marks

Every jump — following a link, `gg`, `G`, `:<n>`, a search moving the view, a
mark — records where you were. `<C-o>` goes back, `<C-i>` forward again, so
after checking a citation `<C-o>` returns exactly to where you were reading.
`ma` remembers the current position as mark `a`; `'a` jumps to it. Positions
are stored as page + offset, so they survive a reload.

### Menus

The menu bar at the top (hide it with `:set nomenubar`) has File, Edit and
View menus. Every entry shows its key — your own binding if you changed it in
the config — so the menus double as a cheat sheet.

From the keyboard: `gm` opens File (also when the bar is hidden; `<F10>`
works too where the OS doesn't take it — on macOS it usually does). `h`/`l`
or ←/→ switch menus, `j`/`k` or ↑/↓ move, Enter or `l` opens a submenu or runs
the entry, `h` leaves a submenu, `Esc` closes. Each entry has an underlined
letter that runs it directly (`gm` `l` `l` `w` = View → Fit width). With the
mouse, click as usual.

- **File**: open, open recent (the files you read last), reload, save
  annotations, save a copy, apply redactions, document info (title, author,
  dates, PDF version, size, annotation counts), quit.
- **Edit**: undo, copy, select text, highlight (each colour), comment,
  annotations, mark for redaction (selection, area, search matches), find,
  find next/previous, edit the config file (opens it in your text editor,
  creating it from the example if needed), reload the config.
- **View**: zoom, fit width/page, recolor, outline, annotations panel, menu
  bar, toolbar, status bar, fullscreen, keys and commands.

### Command line

Press `:` and type a command, then Enter. `Esc` (or Backspace on an empty line)
cancels; Up / Down browse the history.

| Command | Action |
| --- | --- |
| `:<n>` | Go to page n |
| `:open <path>`, `:o`, `:e` | Open another PDF (relative paths are relative to the current file; `~` works) |
| `:set <option> <value>` | Change an option for this session, e.g. `:set zoom.step 1.25`, `:set toolbar`, `:set notoolbar` |
| `:source` | Reload the config file |
| `:help` | Help overlay |
| `:w` | Save annotations into the PDF |
| `:w <path>` | Save a copy with the annotations |
| `:saveas <path>` | Save a copy with the annotations and continue in it |
| `:w!` | Save into a temporary copy anyway (see "PDFs from the browser") |
| `:annotated` | Open the other copy of this paper that has your annotations |
| `:wq`, `:x` | Save and quit |
| `:q` | Quit (refused while there are unsaved annotations) |
| `:q!` | Quit, discarding unsaved annotations |
| `:e!` | Reload the file, discarding unsaved annotations |
| `:annotations` | Annotations panel |
| `:info` | Document properties |
| `:config` | Open the config file in your text editor |
| `:redact-matches` | Mark every match of the current search for redaction |
| `:redact [path]` | Apply all redaction marks, writing `<name>-redacted.pdf` (or `path`) |
| `:<command> [args]` | Any bindable command, e.g. `:zoom_in 3`, `:fit_page` |

### Search

`/` opens the search line at the bottom. The search runs in the background
from the current page to the end and then wraps around, so the first hit shows
up straight away even in long documents; the view jumps to the first match
below the current position.

- Case-insensitive, unless the query contains a capital letter (smart case).
- Whitespace in the query matches any whitespace, including line breaks, and
  words hyphenated across a line break are found whole (`multileptonic`
  matches `multi-` / `leptonic`).
- All matches on screen are highlighted, the current one more strongly.
  `n` / `N` move between them; `Esc` hides the highlights until the next `n`.
- The status bar shows `[match/total]`, with `searching…` while it runs.
- Up / Down in the search line browse previous searches.

### Selecting and copying text

`v` starts visual mode with the caret at the current search match (so
`/words` then `v` starts exactly there), or at the first line on screen. `gv`
puts labels on the words on screen; type one to start there. `V` selects whole
lines. Dragging with the mouse selects too, and a mouse selection continues in
visual mode, so you can refine it from the keyboard (and vice versa with
`Shift`+click).

| Key (visual mode) | Action |
| --- | --- |
| `h` `l` / `j` `k` | Caret left / right, down / up a line (reading order) |
| `w` `b` `e` | Next word, previous word, end of word |
| `0` `^` / `$` | Start / end of line |
| `}` / `{` | End / start of the paragraph |
| `<C-d>` `<C-u>` `<C-f>` `<C-b>` | Scroll; the caret moves with the view |
| `o` | Swap caret and anchor |
| `v` / `V` | Switch char-wise / line-wise, or leave |
| `y`, `Cmd+C` (`Ctrl+C` on Linux) | Copy and leave visual mode |
| `*` | Search for the selected text |
| `/`, `n`, `N` | Search; the caret moves to the match |
| `Esc` | Leave visual mode |

Counts work (`3w`, `5j`). Copied text is cleaned up: lines of a paragraph are
joined with spaces, words hyphenated at a line end are joined back
(`multi-`/`leptonic` → `multileptonic`, while `TeV-`/`scale` keeps its
hyphen), paragraphs are separated by newlines and ligatures are expanded
(ﬁ → fi). Margin line numbers of draft copies are left out.

Known limitations:

- Reading order comes from MuPDF plus some layout heuristics (merging
  fragments of a line, joining a paragraph that continues in the next column,
  splitting paragraphs at indents). Unusual layouts can still jump around.
- Mathematics usually copies poorly: many PDFs encode math glyphs in ways that
  don't map back to sensible characters (e.g. `410þ160 −180` for 410⁺¹⁶⁰₋₁₈₀).
- Scanned PDFs without a text layer have nothing to select or search.

### Highlights, comments, notes and text

These are standard PDF annotations, so Preview, Acrobat, Okular, Zotero and
other viewers show them too.

- Select text (`v` + motions, `/` then `v`, `gv`, or the mouse), then
  `a<letter>` highlights it in that colour: `y` yellow, `g` green, `b` blue,
  `p` pink, `r` red, `o` orange (configurable: `[highlight]` in the config).
  `aa` uses the last colour again.
- `c` on a selection opens a comment box: Enter saves (the selection is
  highlighted with the comment), Shift+Enter starts a new line, Esc cancels.
  A speech bubble next to the line marks a highlight with a comment: hover it
  (or the highlight) to read the comment, click it to edit.
- **Sticky notes** go anywhere on the page: `C` puts a note icon in the middle
  of the view; `h`/`j`/`k`/`l` move it (counts work: `10l`), Enter opens the
  box for its text, Esc cancels. Hover a note to read it, click it to edit.
- **Text boxes** are written on the page itself: `T` places a box (`H`/`L`
  change its width, `+`/`-` the font size), Enter opens the box for the text.
  The text wraps at the box's width and the box grows to fit it. Size and
  colour of new boxes: `[text]` in the config.
- Placing works with the mouse too: drag the box to move it, drag elsewhere
  to draw a new one, double-click to confirm. Or right-click anywhere on a
  page → Add note here / Add text here.
- Notes and text boxes can be dragged to another place with the mouse, or
  moved from the keyboard with `m` in the annotations panel.
- `<C-a>` opens the annotations panel: every annotation in document order with
  its page, the text it covers and its comment. `j`/`k` move, Enter jumps to
  it, `e` edits the comment (or the note's / text box's text), `c<letter>`
  changes the colour, `m` moves a note, text box or redaction mark, `+`/`-`
  change a text box's font size, `d` deletes, `u` undoes, Esc closes.
- **Deleting:** `D` puts a label on every annotation on screen; type one to
  delete it. Or right-click an annotation → Delete (the first entry), or `d`
  in the annotations panel. To delete only a highlight's comment and keep the
  highlight: right-click → Delete comment, or the Delete comment button in
  the comment box. Notes and text boxes have a Delete button in their box
  too. `u` undoes any of these.
- With the mouse: right-click a selection for Highlight (with each colour)
  and Comment…; right-click an annotation to edit it, remove its comment,
  move it, change its colour or text size, or delete it.
- `u` undoes the last annotation change (repeatedly).
- Changes are kept in memory until `:w`, which writes them into the PDF as an
  incremental update (the original bytes stay unchanged; the update is
  appended). The status bar shows `[+]` after the file name while there are
  unsaved changes. `:q`, opening another file and reloading are refused until
  you save (`:w`) or discard (`:q!`, `:e!`); closing the window warns once.
- If the file changes on disk while you have unsaved annotations, vimpdf
  doesn't reload it but tells you; its own saves never trigger a reload.

### PDFs from the browser

When a browser opens a PDF in vimpdf, it first downloads it, so every click
gives a new copy. vimpdf copes with that:

- **Your place follows the paper, not the file.** Each document is recognised
  by the permanent ID inside the PDF (or a hash of the file), so a fresh copy
  of a paper you've read opens where you left off.
- **Annotations are never lost to a temporary copy.** If the PDF lives in a
  temporary folder (where browsers put files they hand to other apps), `:w`
  refuses and explains; `:saveas ~/Papers/name.pdf` saves a permanent copy
  with your highlights and continues there (`:w!` saves into the temporary
  file anyway). The first highlight in such a file shows a reminder.
- **Back to your annotated copy:** opening another copy of a paper you
  annotated says so — `:annotated` (or File → Open annotated copy) switches to
  it.

To keep Firefox's Downloads folder clean, set these in `about:config` (they
only affect files opened with another application, not normal downloads):
`browser.download.start_downloads_in_tmp_dir = true` and
`browser.helperApps.deleteTempFileOnExit = true`.

### Redaction (censoring)

Redaction permanently removes content. vimpdf never applies it to the original
file: it always writes a new one.

1. Mark what to remove — marks are red boxes, and they're annotations like the
   others (listed in `<C-a>`, removable with `d`, undoable with `u`):
   - text: select it (`v` + motions, or the mouse) and press `x`;
   - every occurrence of a word or name: search it (`/name`), then
     `:redact-matches`;
   - an area (figure, image, signature): `X` puts a rectangle in the middle of
     the view; `h`/`j`/`k`/`l` move it, `H`/`J`/`K`/`L` resize it (counts work:
     `10L`), Enter marks it, Esc cancels — or drag a rectangle with the mouse.
2. `:redact` applies every mark to a copy of the document and writes
   `<name>-redacted.pdf` next to it (`:redact <path>` chooses the name; an
   existing file is never overwritten, the original never). Under each mark,
   text is removed, image pixels are blanked, vector drawings that are fully
   covered are removed, and a black box is drawn. The file is rewritten from
   scratch with garbage collection, so nothing removed survives inside it.
3. vimpdf then re-reads the written file and checks that no text is left under
   any mark, reports the result, and opens the redacted file (if your only
   unsaved changes were the marks).

**Check before sharing.** Search-based marking only finds real text. Text that
is part of a figure (labels drawn as shapes, or inside an image) and scanned
pages are invisible to search: look through the document and cover them with
area marks (`X`). Document metadata (title, author, keywords in the file's
properties) is not changed.

### Reading comfort

- **Recolor** (`<C-r>`): pages are drawn with light text on a dark background
  (`colors.recolor_dark` / `colors.recolor_light`). `ui.recolor = true` starts
  in this mode.
- **Outline** (`<Tab>`): the document's table of contents, opened at the
  section you're reading. `j`/`k` (or arrows, `<C-d>`/`<C-u>`, `g`/`G`) move,
  Enter (or `l`, `o`, Space) jumps and closes, `Esc`/`<Tab>`/`q`/`h` close.
  Clicking an entry jumps and keeps the panel open. Jumps go into the
  jumplist (`<C-o>` returns).
- **Remembered position**: the page, offset, zoom, fit mode and marks of every
  file are saved (on quit, when opening another file, and every few seconds)
  in `~/.local/state/vimpdf/` (or `$XDG_STATE_HOME/vimpdf/`) and restored when the
  file is opened again.
- **Auto-reload**: when the file changes on disk (e.g. LaTeX recompiles it),
  the view reloads in place once the file has stopped changing for 300 ms,
  keeping the position. `ui.auto_reload = false` turns it off.

### Status bar

From left to right: mode, file name (or the latest message — errors in red),
then on the right the search position (`[3/27]`), the keys typed so far
(e.g. `12g`), `page/total` and the zoom.
The page shown is the one at the centre of the window.

## Configuration

`~/.config/vimpdf/config.toml` (also on macOS), or `$XDG_CONFIG_HOME/vimpdf/config.toml`.
The file isn't created for you; to start from the documented example:

```fish
mkdir -p ~/.config/vimpdf
cp config.example.toml ~/.config/vimpdf/config.toml
```

A missing file means defaults. Invalid entries are reported in the status bar
and the log, and the default is kept — a bad config never stops the viewer.
`:source` reloads the file without restarting.

```toml
[ui]
toolbar = true

[zoom]
step = 1.2
default_fit = "page"

[keys.normal]
"x" = "scroll_down 3"
"<C-F>" = "toggle_fullscreen"
"q" = ""                 # unbind
```

See [docs/configuration.md](docs/configuration.md) for every option and every
command name, and [config.example.toml](config.example.toml) for a starting point.

## Troubleshooting

- Logging: `RUST_LOG=vimpdf=debug vimpdf paper.pdf`. `vimpdf=trace` also logs every
  command, every scroll position and every frame where a page had no render yet.
- Config warnings are listed in full in the log; the status bar shows the first.

## Tests

```fish
cargo test
# Rendering tests against a real PDF (PDFs are not committed to the repo):
env VIMPDF_TEST_PDF=paper.pdf cargo test -- --ignored
```

## License

AGPL-3.0-or-later (required by MuPDF). See [LICENSE](LICENSE).
