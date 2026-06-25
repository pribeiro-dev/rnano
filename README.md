# rnano (working title)

A minimal, fast, cross‑platform TUI text editor inspired by **nano**. Goals:
instant startup, reliable saves, correct Unicode, and a small dependency
footprint.

> Status: **early but working.** The M1 walking skeleton is in place — you can
> open a file, edit it, and save it. Search, undo/redo, and config are next.

## Build & run

```sh
cargo build --release
./target/release/rnano [FILE]
```

Options:

| Flag | Meaning |
| --- | --- |
| `-v`, `--readonly` | Open read‑only (no edits, no saving) |
| `-B`, `--backup` | Write a backup to `<FILE>~` before overwriting |
| `-V`, `--version` | Print version |
| `-h`, `--help` | Print help |

## Keys

| Key | Action |
| --- | --- |
| Arrows / Home / End / PgUp / PgDn | Move |
| `Ctrl+A` / `Ctrl+E` | Start / end of line |
| `Ctrl+O` | Write file (prompts for a name) |
| `Ctrl+S` | Save to the current file |
| `Ctrl+K` / `Ctrl+U` | Cut line / paste |
| `Ctrl+C` | Show cursor position |
| `Ctrl+G` | Help |
| `Ctrl+X` | Exit (prompts if there are unsaved changes) |

## Design notes

- **Buffer:** [`ropey`](https://crates.io/crates/ropey) rope, with line endings
  normalized to `\n` in memory. CRLF is detected on load and restored on save.
- **Safe saves:** content is written to a temp file in the same directory,
  `fsync`ed, and `rename`d over the target, preserving its permissions.
- **Unicode:** display columns use `unicode-width`; tabs expand to the next tab
  stop. Files with NUL bytes or non‑UTF‑8 content are rejected (legacy encodings
  are a later milestone).

## Roadmap & Milestones
- **M1 — MVP:** open/edit/save, navigation, status bar, prompts, safe writes. ✅ (skeleton)
- **M2 — QoL:** incremental search, undo/redo, soft‑wrap, line numbers, config, help.
- **M3 — Robustez:** encodings, autosave/swap, large files, long lines, performance.
- **M4 — Syntax:** light highlighting & themes, fast startup preserved.
- **M5 — Ergonomia:** clipboard, mouse, multiple buffers, macros, on‑save hooks.

## Build status
CI runs fmt, clippy (`-D warnings`), build, and tests on Linux/macOS/Windows.

## License
MIT — see [LICENSE](LICENSE).
