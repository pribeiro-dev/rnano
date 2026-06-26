# rnano — Issues Checklist

## M0 — Planning & Infra
- [ ] Scaffold repo & initial docs
- [ ] Decide final project name
- [ ] Config format & location policy
- [ ] Keymap policy
- [ ] CI guard & release targets

## M1 — MVP
- [x] Load file (UTF‑8, CRLF/LF, binary guard)
- [x] Buffer core + edits + tabs
- [x] Viewport & cursor + scroll
- [x] Status bar
- [x] Prompts (write, confirm quit) — goto line still TODO
- [x] Atomic save + backup ~ + CRLF + perms
- [x] Keybindings nano‑like (core subset)
- [x] Readonly + friendly errors
- [x] Tests: unit (property tests still TODO)

## M2 — QoL
- [ ] Incremental search
- [ ] Undo/redo ring
- [ ] Soft wrap
- [ ] Line numbers
- [ ] Config load
- [ ] Help screen

## M3 — Robustez & Reais
- [ ] Encodings legacy (opt‑in)
- [ ] Autosave/swap
- [ ] Large files
- [ ] Very long lines
- [ ] Cross‑platform EOL/perms tests
- [ ] Perf marks & benches

## M4 — Sintaxe & Temas
- [ ] Lightweight syntax rules
- [ ] Themes
- [ ] Disable highlight over N MB

## M5 — Ergonomia
- [ ] Clipboard
- [ ] Mouse (optional)
- [ ] Multiple buffers
- [ ] Macros simples
- [ ] On‑save hooks
