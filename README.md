# rnano

A minimal, fast, cross-platform terminal text editor inspired by **GNU nano**.

**Goals:** instant startup · trustworthy saves · correct Unicode · small binary

[![CI](https://github.com/pribeiro-dev/rnano/actions/workflows/ci.yml/badge.svg)](https://github.com/pribeiro-dev/rnano/actions/workflows/ci.yml)

---

## Install

```sh
cargo install --path .
```

Or build from source:

```sh
cargo build --release
# binary at target/release/rnano
```

Requires Rust 1.85+. No system dependencies.

---

## Usage

```
rnano [OPTIONS] [FILE [FILE ...]]
```

Open multiple files and switch between them with `Alt+N` / `Alt+B`.

| Option | Description |
|--------|-------------|
| `-v`, `--readonly` | Open all files read-only |
| `-B`, `--backup` | Write `<file>~` backup before saving |
| `-V`, `--version` | Print version and exit |
| `-h`, `--help` | Print help and exit |

---

## Keys

### Navigation

| Key | Action |
|-----|--------|
| Arrow keys | Move cursor |
| `Home` / `End` | Start / end of line |
| `Ctrl+A` / `Ctrl+E` | Start / end of line |
| `PgUp` / `PgDn` | Page up / down |
| Click (mouse) | Move cursor to position |
| Scroll wheel | Scroll viewport |

### Editing

| Key | Action |
|-----|--------|
| `Ctrl+K` | Cut line |
| `Ctrl+U` | Paste from kill ring |
| `Alt+W` | Copy kill ring to OS clipboard |
| `Alt+U` | Paste from OS clipboard |
| `Ctrl+Z` | Undo |
| `Ctrl+Y` | Redo |
| `Tab` | Insert tab |
| `Enter` | Insert newline |
| `Backspace` / `Delete` | Delete character |

### File

| Key | Action |
|-----|--------|
| `Ctrl+O` | Write / save as (prompts for filename) |
| `Ctrl+S` | Save to current filename |
| `Ctrl+X` | Exit (prompts if unsaved changes) |

### Search

| Key | Action |
|-----|--------|
| `Ctrl+W` | Open incremental search |
| `Ctrl+W` *(in search)* | Next match |
| `Ctrl+Q` *(in search)* | Previous match |
| `Alt+C` *(in search)* | Toggle case sensitivity |
| `Enter` | Accept and jump to match |
| `Esc` | Cancel search, restore cursor |

### View

| Key | Action |
|-----|--------|
| `Ctrl+N` | Toggle line numbers |
| `Ctrl+P` | Toggle soft wrap |
| `Ctrl+G` | Help screen (↑↓ to scroll, any key closes) |
| `Ctrl+C` | Show cursor position |

### Buffers

| Key | Action |
|-----|--------|
| `Alt+N` | Next buffer |
| `Alt+B` | Previous buffer |

### Macros

| Key | Action |
|-----|--------|
| `Ctrl+R` | Start / stop recording macro |
| `Alt+R` | Play back last macro |

---

## Config file

rnano reads a config file on startup:

| Platform | Path |
|----------|------|
| Linux / macOS | `$XDG_CONFIG_HOME/rnano/config.toml` (default: `~/.config/rnano/config.toml`) |
| Windows | `%APPDATA%\rnano\config.toml` |

CLI flags override config file values.

### Options

```toml
tab_width         = 4      # display columns per tab stop
line_numbers      = false  # show line-number gutter
soft_wrap         = false  # wrap long lines at viewport width
backup            = false  # write <file>~ before saving
undo_depth        = 1000   # max undo history entries
autosave_interval = 0      # autosave interval in seconds (0 = off)
highlight_max_kb  = 512    # disable syntax highlighting above N KiB
syntax            = true   # enable syntax highlighting
theme             = "dark" # "dark" or "light"
on_save_hook      = ""     # shell command after save; %f = file path
                           # example: on_save_hook = "rustfmt %f"
```

---

## Features

**Editing**
- Nano-like keybindings; most muscle memory transfers directly
- Undo/redo with configurable depth; consecutive single-char inserts coalesce into one step
- Cut/paste kill ring; OS clipboard via `xclip`/`xsel` (Linux), `pbcopy`/`pbpaste` (macOS), `clip.exe` (Windows)
- Session macros: record a key sequence and replay it

**Files**
- Atomic saves: write to temp → `fsync` → `rename`, preserving file permissions
- Optional `~` backup before overwriting
- CRLF detected on load, normalized to `\n` in memory, restored on save
- Binary and non-UTF-8 files rejected with actionable error messages
- Autosave to `.filename.swp` at a configurable interval

**Display**
- Syntax highlighting for Rust, TOML, JSON, Markdown, Shell, Python
- Dark and light colour themes
- Optional line-number gutter (dynamic width)
- Soft-wrap toggle (disables horizontal scroll)
- Unicode-aware: display widths via `unicode-width`, tab stop expansion
- Highlighting auto-disabled for files above `highlight_max_kb`

**Navigation**
- Mouse support: click to position cursor, scroll wheel to pan viewport
- Multiple buffers from the command line; ring-style switching with `Alt+N`/`Alt+B`
- Incremental search with match highlighting and case-insensitive toggle

**Automation**
- `on_save_hook`: run any shell command after each successful save (e.g. `rustfmt %f`, `prettier --write %f`)

---

## Design

| Concern | Approach |
|---------|----------|
| Buffer | [`ropey`](https://crates.io/crates/ropey) rope (LF-only mode); O(log n) edits |
| Terminal | [`crossterm`](https://crates.io/crates/crossterm) — Linux, macOS, Windows |
| Display widths | [`unicode-width`](https://crates.io/crates/unicode-width) |
| Config parsing | Hand-rolled TOML subset — no extra crate |
| Clipboard | Subprocess (`xclip`/`pbcopy`/`clip`) — no extra crate |
| Saves | Atomic: temp file → fsync → rename |
| Line endings | CRLF ↔ LF converted at the I/O boundary only |

---

## Roadmap

| Milestone | Status |
|-----------|--------|
| **M1** — open · edit · save · navigate · status bar | ✅ Done |
| **M2** — search · undo/redo · config · line numbers · soft wrap · help | ✅ Done |
| **M5** — clipboard · mouse · multiple buffers · macros · on-save hooks | ✅ Done (preview) |
| **M3** — legacy encodings · streaming large files | Planned |
| **M4** — extended syntax highlighting (multi-line states, more languages) | Planned |

---

## Development

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release
cargo bench          # criterion benchmarks (benches/buffer.rs)
```

CI runs `fmt`, `clippy -D warnings`, `build`, `test`, and `release build` on Linux, macOS, and Windows against the stable toolchain.

---

## License

MIT — see [LICENSE](LICENSE).
