# rnano

> A fast, friendly terminal text editor for people who just want to edit a file.

[![CI](https://github.com/pribeiro-dev/rnano/actions/workflows/ci.yml/badge.svg)](https://github.com/pribeiro-dev/rnano/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

rnano is a nano-inspired editor that starts instantly, saves reliably, and gets out of your way. If you know nano, you already know rnano — the keybindings feel the same, but it adds undo/redo, syntax highlighting, multiple buffers, a config file, and mouse support without getting bloated.

```
 rnano  src/main.rs *                                                   
  1 mod buffer;                                                          
  2 mod clipboard;                                                       
  3 mod config;                                                          
  4 mod editor;                                                          
  5 mod highlight;                                                       
  6 mod history;                                                         
  7                                                                      
  8 use std::path::PathBuf;                                              
  9 use std::process::ExitCode;                                          
 10                                                                      
 Ln 1, Col 1   LF  [1/3]                                                
 ^O Write  ^X Exit  ^G Help  ^K Cut  ^U Paste  ^W Search  ^Z Undo      
```

---

## Why rnano?

- **It starts instantly.** No Electron, no LSP, no plugin ecosystem to boot.
- **It's hard to break.** Saves are atomic (temp → fsync → rename). Your file is never half-written.
- **It handles your files.** CRLF on Windows, LF on Unix — it detects, normalises, and restores.
- **It won't surprise you.** If you've used nano, the muscle memory is already there.

---

## Install

```sh
cargo install --git https://github.com/pribeiro-dev/rnano
```

Or clone and build:

```sh
git clone https://github.com/pribeiro-dev/rnano
cd rnano
cargo build --release
./target/release/rnano myfile.txt
```

Requires Rust 1.85+. Runs on Linux, macOS, and Windows. No system libraries required.

---

## What it can do

**Edit**
Undo and redo. Cut and paste. A kill ring that actually works. Type `Ctrl+Z`, get your change back. Simple.

**Search**
`Ctrl+W` opens incremental search — results highlight as you type. Jump forward and back through matches. Toggle case-sensitivity mid-search with `Alt+C`.

**Multiple files**
Open several files at once and switch between them with `Alt+N` and `Alt+B`. The status bar shows which buffer you're on.

**Syntax highlighting**
Rust, TOML, JSON, Markdown, Shell, and Python out of the box. Automatically disabled for large files so it never slows you down.

**Mouse**
Click to place your cursor. Scroll to pan. Works in most terminals.

**Macros**
`Ctrl+R` to start recording, `Ctrl+R` again to stop, `Alt+R` to replay. Good for repetitive edits.

**Config**
A single TOML file at `~/.config/rnano/config.toml`. Set your tab width, enable line numbers, pick a theme, or wire up a post-save hook (like `rustfmt %f`).

---

## Quick reference

| Key | What it does |
|-----|-------------|
| `Ctrl+O` | Save (prompts for filename if new) |
| `Ctrl+S` | Save to current file |
| `Ctrl+X` | Quit |
| `Ctrl+W` | Search |
| `Ctrl+K` / `Ctrl+U` | Cut / paste line |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| `Ctrl+G` | Help |
| `Ctrl+N` | Toggle line numbers |
| `Ctrl+P` | Toggle soft wrap |
| `Alt+N` / `Alt+B` | Next / previous buffer |
| `Ctrl+R` / `Alt+R` | Record / play macro |
| `Alt+W` / `Alt+U` | Copy / paste OS clipboard |

Press `Ctrl+G` inside the editor for the full list.

---

## Config

`~/.config/rnano/config.toml` (Linux/macOS) · `%APPDATA%\rnano\config.toml` (Windows)

```toml
tab_width         = 4
line_numbers      = false
soft_wrap         = false
backup            = false
theme             = "dark"   # or "light"
autosave_interval = 0        # seconds; 0 = off
on_save_hook      = ""       # e.g. "rustfmt %f"
```

---

## License

MIT — see [LICENSE](LICENSE).
