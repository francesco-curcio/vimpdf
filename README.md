# pdfv

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Everything works from the keyboard; the mouse works too
when you want it. Rendering is done by [MuPDF](https://mupdf.com/); the window
and UI use [egui/eframe](https://github.com/emilk/egui).

Status: in development — see [PLAN.md](PLAN.md) for the roadmap. Done so far:
continuous scrolling, vim motions with counts, links, zoom and fit modes,
status bar, `:` command line, config file, toolbar, help overlay, search,
link hints, jumplist and marks, text selection with vim visual mode and the
mouse.

Documentation:

- this README — building, running, keys, mouse, command line
- [docs/configuration.md](docs/configuration.md) — every config option
- [config.example.toml](config.example.toml) — a commented example config
- [docs/architecture.md](docs/architecture.md) — how the code is organised

## Building

Requirements (macOS):

- Xcode Command Line Tools (`xcode-select --install`), needed to compile MuPDF.
- Rust stable via [rustup](https://rustup.rs/). On Apple Silicon make sure the
  toolchain is native: `rustc -vV` should print `host: aarch64-apple-darwin`.
  An x86_64 toolchain builds an Intel binary that runs under Rosetta.

```fish
cargo build --release
```

The first build compiles MuPDF from source and takes a minute or two.

To run it from anywhere as `pdfv`:

```fish
cargo install --path .
fish_add_path ~/.cargo/bin   # once, if ~/.cargo/bin isn't on your PATH
```

## Usage

```fish
pdfv paper.pdf
pdfv --config other.toml paper.pdf   # use a different config file
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
| `:` | Command line |
| `?` | Help overlay: active keys, mouse actions, commands |
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
| Pinch, or `Cmd` + wheel | Zoom, anchored at the pointer | `+` `-` |
| Click a link | Follow it | `f` + label |
| Drag over text | Select it | `v` + motions |
| Double / triple click | Select a word / a line | `gv` + label, `V` |
| `Shift` + click | Extend the selection | motions in visual mode |
| `Alt` + drag, middle drag | Pan | `h` `j` `k` `l` |
| Right click | Menu: copy, search for selection, open link, copy link address | `y`, `*`, `f`, `yf` |
| Click the page number in the status bar | Go to page | `:<n>` |
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
| `:q` | Quit |
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
| `y`, `Cmd+C` | Copy and leave visual mode |
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

### Status bar

From left to right: mode, file name (or the latest message — errors in red),
then on the right the search position (`[3/27]`), the keys typed so far
(e.g. `12g`), `page/total` and the zoom.
The page shown is the one at the centre of the window.

## Configuration

`~/.config/pdfv/config.toml` (also on macOS), or `$XDG_CONFIG_HOME/pdfv/config.toml`.
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

- Logging: `RUST_LOG=pdfv=debug pdfv paper.pdf`. `pdfv=trace` also logs every
  command, every scroll position and every frame where a page had no render yet.
- Config warnings are listed in full in the log; the status bar shows the first.

## Tests

```fish
cargo test
# Rendering tests against a real PDF (PDFs are not committed to the repo):
env PDFV_TEST_PDF=paper.pdf cargo test -- --ignored
```

## License

AGPL-3.0-or-later (required by MuPDF). See [LICENSE](LICENSE).
