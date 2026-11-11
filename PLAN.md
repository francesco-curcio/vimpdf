# vimpdf — plan

A keyboard-driven, vim-style PDF viewer written in Rust, tuned for reading
scientific papers. It sits between zathura and okular: zathura's
keyboard-first, minimal UI, with okular's mouse support and discoverability
available when wanted.

The original milestones (M0–M11: scrolling and motions, zoom, command line and
config, search, links and jumplist, visual mode, reading comfort, macOS
integration, annotations, redaction, menu bar) are all done; their notes are in
the git history (this file before 2026-11-11). User-facing documentation is in
`README.md`, `docs/configuration.md` and `config.example.toml`; code structure
in `docs/architecture.md`.

## Goals

- Read a PDF entirely from the keyboard, with vim-like motions.
- Everything is also reachable with the mouse/trackpad.
- Smooth continuous scrolling, sharp rendering on HiDPI displays at any zoom.
- Never block the UI thread on rendering, text extraction or search.
- Fully configurable keybindings and appearance through a plain-text config.
- Excellent for papers: fast search, link following, quick jump-and-return to
  references/figures/equations, remembered reading position.

Non-goals: writing a PDF renderer (MuPDF does that), editing page content,
form filling, printing, Windows support.

## Not yet checked by a human

- On Linux: the visual appearance (bars, help, outline panel, highlight
  colours, recolor), pinch-to-zoom on a real trackpad, and choosing vimpdf in
  Firefox's settings.
- Not available on Linux with winit 0.30: pinch zoom, file drop on Wayland;
  opening a file from outside starts a new window.

## Later ideas (not scheduled)

- Faster first render of very heavy pages (e.g. a low-resolution preview), so
  even extreme-speed scrolling never shows a blank page.
- Two-page / side-by-side mode.
- Copy the text inside a drawn box (copying a box as an image is `Y`).
- SyncTeX forward/backward search.

## Working conventions for the agent

- Prefer small, focused modules and plain data types between threads.
- Don't add dependencies without a short justification in the commit message.
- Ask the user to provide or point to a real PDF for testing (an arXiv paper
  is ideal); don't commit PDFs to the repo.
- When a library API is unclear, read its docs/source rather than guessing.
- Keep the README and `docs/` up to date with keys, mouse actions and config
  options.
- The user's shell is fish; any shell commands given to the user should be
  fish-compatible.
