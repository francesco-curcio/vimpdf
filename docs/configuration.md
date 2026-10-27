# Configuration

pdfv reads `~/.config/pdfv/config.toml` at startup (`$XDG_CONFIG_HOME/pdfv/config.toml`
if `XDG_CONFIG_HOME` is set; the same path on macOS, rather than
`~/Library/Application Support`). Use `pdfv --config <path>` for another file.

- A missing file means all defaults.
- Every invalid entry produces a warning (status bar + log) and keeps its
  default. A TOML syntax error ignores the whole file. The viewer always starts.
- `:source` reloads the file, including key bindings, without restarting.
- `:set <option> <value>` changes an option for the current session only.

## Options

`:set` accepts the full name (`zoom.step`) or just the last part when it is
unambiguous (`toolbar`). Booleans also accept `:set toolbar` / `:set notoolbar`.

### `[ui]`

| Option | Default | Description |
| --- | --- | --- |
| `font_family` | `"Menlo"` | Font of the status bar, command line and help. Looked up by file name in the system and user font folders (`"JetBrains Mono"` finds `JetBrainsMono-Regular.ttf`). Falls back to egui's built-in monospace font with a warning. |
| `font_size` | `14` | Size of that font, 6–72. Also scales the bars. |
| `statusbar` | `true` | Show the status bar. |
| `toolbar` | `false` | Show the toolbar (`<C-t>` toggles it). |
| `start_maximised` | `true` | Open the window filling the screen's usable area. |

### `[colors]`

Colours are `"#rrggbb"` or `"#rrggbbaa"`.

| Option | Default | Used for |
| --- | --- | --- |
| `page_background` | `"#ffffff"` | Page colour shown while a page is still rendering. |
| `recolor_dark` | `"#1e1e1e"` | Background in recolor (dark) mode — M7. |
| `recolor_light` | `"#d4d4d4"` | Text in recolor mode — M7. |
| `search_highlight` | `"#f9e2af80"` | Search matches — M4. |
| `selection` | `"#89b4fa60"` | Text selection — M6. |
| `caret` | `"#f38ba8"` | Visual-mode caret — M6. |
| `hint_background` | `"#fab387"` | Link / word hint labels — M5. |

### `[scroll]`

| Option | Default | Description |
| --- | --- | --- |
| `step` | `60` | Distance of one `j`/`k`/`h`/`l`, in screen points. |
| `smooth` | `true` | Animate keyboard scrolling. Jumps (`gg`, `J`, links, …) are always instant. |

### `[zoom]`

| Option | Default | Description |
| --- | --- | --- |
| `step` | `1.1` | Zoom factor of one `+`/`-`, 1.01–4. |
| `default_fit` | `"width"` | Fit mode for newly opened files: `"width"`, `"page"` or `"none"` (100%). |

### `[mouse]`

| Option | Default | Description |
| --- | --- | --- |
| `zoom_modifier` | `"cmd"` | Modifier that turns the wheel into zoom: `"cmd"`, `"ctrl"`, `"alt"` or `"shift"`. Pinch always zooms. |
| `drag` | `"select"` | What dragging on a page does: `"select"` text or `"pan"` the view. |
| `pan_modifier` | `"alt"` | Holding it swaps the drag behaviour (pans when `drag = "select"`, selects when `"pan"`). Middle-drag always pans. |
| `copy_on_select` | `false` | Copy mouse selections to the clipboard as soon as the drag ends. |

Options marked with a milestone are accepted now and take effect when that
feature lands.

## Key bindings

```toml
[keys.normal]
"x" = "scroll_down 3"
"<C-F>" = "toggle_fullscreen"
"gp" = "goto_page 1"
"q" = ""            # an empty command unbinds the key
```

Keys use vim notation: printable characters as themselves (`"G"`, `"+"`), and
`<...>` for the rest: `<C-d>` (Ctrl+d), `<C-F>` (Ctrl+Shift+f — an uppercase
letter implies Shift), `<A-x>` (Alt/Option), `<D-x>` (Cmd), `<S-Space>`,
`<Space>`, `<Esc>`, `<Tab>`, `<CR>`, `<BS>`, `<Up>`/`<Down>`/`<Left>`/`<Right>`,
`<Home>`, `<End>`, `<PageUp>`, `<PageDown>`, `<F1>`…`<F12>`, `<lt>` for `<`.
Sequences combine them: `"gg"`, `"m<Tab>"`. A count typed before a binding
multiplies it (`5x` with the example above scrolls 15 steps).

There are two modes with their own bindings: `[keys.normal]` and
`[keys.visual]` (text selection).

```toml
[keys.visual]
"Y" = "yank"
"H" = "line_start"
"L" = "line_end"
```

### Commands

These names are used in `[keys.*]` and can also be typed after `:`.
`[n]` is an optional count (default 1).

| Command | Default keys |
| --- | --- |
| `scroll_down [n]`, `scroll_up [n]` | `j` `<Down>`, `k` `<Up>` |
| `scroll_left [n]`, `scroll_right [n]` | `h` `<Left>`, `l` `<Right>` |
| `half_page_down [n]`, `half_page_up [n]` | `<C-d>`, `<C-u>` |
| `screen_down [n]`, `screen_up [n]` | `<C-f>` `<Space>` `<PageDown>`, `<C-b>` `<S-Space>` `<PageUp>` |
| `next_page [n]`, `prev_page [n]` | `J`, `K` |
| `goto_first`, `goto_last` | `gg` `<Home>`, `G` `<End>` |
| `goto_page <n>` | — |
| `zoom_in [n]`, `zoom_out [n]`, `zoom_reset` | `+`, `-`, `=` |
| `fit_width`, `fit_page` | `s`, `a` |
| `command_line [text]` | `:` |
| `search_prompt [text]` | `/` |
| `search <text>` | — (`:search foo` searches without opening the prompt) |
| `search_next [n]`, `search_prev [n]` | `n`, `N` |
| `hints`, `hints_copy` | `f`, `yf` |
| `visual`, `visual_line` | `v`, `V` (also in visual mode: switch or leave) |
| `word_hints` | `gv` |
| `jump_back [n]`, `jump_forward [n]` | `<C-o>`, `<C-i>` |
| `set_mark [letter]`, `goto_mark [letter]` | `m`, `'` and `` ` `` (without a letter they wait for the next key) |
| `toggle_toolbar`, `toggle_statusbar` | `<C-t>`, — |
| `toggle_help` | `?` |
| `toggle_fullscreen` | `<C-F>` |
| `reload` | `r` |
| `source` | — |
| `open <path>` | — |
| `cancel` | `<Esc>` |
| `quit` | `q` |

Visual mode only:

| Command | Default keys |
| --- | --- |
| `caret_left [n]`, `caret_right [n]` | `h` `<Left>`, `l` `<Right>` |
| `caret_down [n]`, `caret_up [n]` | `j` `<Down>`, `k` `<Up>` |
| `word_forward [n]`, `word_backward [n]`, `word_end [n]` | `w`, `b`, `e` |
| `line_start [n]`, `line_end [n]` | `0` `^`, `$` |
| `block_forward [n]`, `block_backward [n]` | `}`, `{` |
| `swap_ends` | `o` |
| `yank` | `y`, `Cmd+C` |
| `search_selection` | `*` |
| also: `half_page_down/up`, `screen_down/up`, `search_prompt`, `search_next/prev`, `command_line`, `toggle_help`, `cancel` | `<C-d>` `<C-u>` `<C-f>` `<C-b>` `/` `n` `N` `:` `?` `<Esc>` |
