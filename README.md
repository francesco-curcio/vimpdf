# pdfv

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Everything works from the keyboard; the mouse works too
when you want it. Rendering is done by [MuPDF](https://mupdf.com/); the window
and UI use [egui/eframe](https://github.com/emilk/egui).

Status: in development — see [PLAN.md](PLAN.md) for the roadmap. Done so far:
continuous scrolling, vim motions with counts, links, zoom and fit modes,
status bar, `:` command line, config file, toolbar, help overlay, search.

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
| Click a link | Follow it | `f` (coming in M5) |
| Click the page number in the status bar | Go to page | `:<n>` |
| Toolbar buttons | Zoom, fit, go to page, help | `+` `-` `s` `a` `:<n>` `?` |

Internal links jump to their target page. External links open in the default
browser — only `http(s)` and `mailto` links, so a PDF can't use a link to
launch local files or applications.

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
