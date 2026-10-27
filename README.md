# pdfv

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. Rendering is done by [MuPDF](https://mupdf.com/); the window
and UI use [egui/eframe](https://github.com/emilk/egui).

Status: early development (milestone M0 — opens a PDF and shows page 1). See
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

| Key | Action |
| --- | --- |
| `q` | Quit |

More keybindings arrive with the next milestones.

Logging: `RUST_LOG=pdfv=debug ./target/release/pdfv paper.pdf`

## Tests

```fish
cargo test
# Rendering test against a real PDF (PDFs are not committed to the repo):
env PDFV_TEST_PDF=paper.pdf cargo test -- --ignored
```

## License

AGPL-3.0-or-later (required by MuPDF). See [LICENSE](LICENSE).
