# pdfv

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Rendering is done by [MuPDF](https://mupdf.com/); the window
and UI use [egui/eframe](https://github.com/emilk/egui).

Status: early development (milestone M1 — continuous scrolling, vim motions,
links). See
[PLAN.md](PLAN.md) for the roadmap.

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

## Usage

```fish
./target/release/pdfv paper.pdf
```

Counts work as in vim: `5j`, `3J`, `12G`.

| Key | Action |
| --- | --- |
| `j` / `k`, `↓` / `↑` | Scroll down / up |
| `h` / `l`, `←` / `→` | Scroll left / right (when the page is wider than the window) |
| `<C-d>` / `<C-u>` | Half screen down / up |
| `<C-f>` / `<C-b>`, `Space` / `Shift-Space`, `PageDown` / `PageUp` | Full screen down / up |
| `J` / `K` | Next / previous page (snaps to page top) |
| `gg` / `G`, `Home` / `End` | First page / end of document; `<n>G` or `<n>gg` goes to page n |
| `Esc` | Cancel a pending count or key sequence |
| `q` | Quit |

| Mouse | Action |
| --- | --- |
| Wheel / two-finger scroll | Scroll |
| Click a link | Follow it (internal links go to the target page; web and mail links open in the default browser) |

Logging: `RUST_LOG=pdfv=debug ./target/release/pdfv paper.pdf` (`pdfv=trace`
also logs every command and blank-page frame).

## Tests

```fish
cargo test
# Rendering test against a real PDF (PDFs are not committed to the repo):
env PDFV_TEST_PDF=paper.pdf cargo test -- --ignored
```

## License

AGPL-3.0-or-later (required by MuPDF). See [LICENSE](LICENSE).
