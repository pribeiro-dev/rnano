#!/usr/bin/env bash
set -euo pipefail

# Usage: run from repository root after pushing to GitHub
# Requires: gh (https://cli.github.com/) authenticated
# Also requires: jq

# Detect owner/repo from git remote
REPO=${REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
if [[ -z "${REPO}" ]]; then
  echo "Cannot detect repo. Set REPO=owner/name and retry." >&2
  exit 1
fi
OWNER=${OWNER:-${REPO%%/*}}

say() { printf "[1;36m==> %s[0m
" "$*"; }

say "Using repo: ${REPO} (owner: ${OWNER})"

# --- (1) Create Project (if missing) ---
PROJECT_TITLE=${PROJECT_TITLE:-"rnano Roadmap"}
PROJECT_NUMBER=${PROJECT_NUMBER:-}

ensure_project() {
  if [[ -n "${PROJECT_NUMBER}" ]]; then
    say "Using provided project number: ${PROJECT_NUMBER}"
    return
  fi
  say "Ensuring GitHub Project exists: '${PROJECT_TITLE}'"
  # Try to find existing by title
  existing=$(gh project list --owner "${OWNER}" --format json | jq -r --arg t "${PROJECT_TITLE}" '.projects[] | select(.title==$t) | .number' || true)
  if [[ -n "${existing}" ]]; then
    PROJECT_NUMBER="$existing"
    say "Found existing project #${PROJECT_NUMBER}"
    return
  fi
  # Create new project and capture number
  PROJECT_NUMBER=$(gh project create --owner "${OWNER}" --title "${PROJECT_TITLE}" --format json | jq -r '.number')
  say "Created project #${PROJECT_NUMBER} — switch its view to Board in the GitHub UI if you prefer"
}

# --- (2) Milestones ---
create_milestone() {
  local title="$1"; shift
  local description="$*"
  gh api --silent \
    --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/milestones" \
    -f title="$title" -f state=open -f description="$description" || true
}

# --- (3) Labels ---
create_label() {
  local name="$1" color="$2" description="$3"
  gh api --silent --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/labels" \
    -f name="$name" -f color="$color" -f description="$description" || true
}

# --- (4) Issues ---
# Helper to create an issue attached to a milestone and with labels
issue() {
  local title="$1" milestone="$2" labels="$3" body="$4"
  gh issue create --repo "$REPO" \
    --title "$title" \
    --milestone "$milestone" \
    --label $(echo "$labels" | sed 's/,/ --label /g') \
    --body "$body" >/dev/null
}

# --- (5) Add all repo issues to Project ---
add_all_issues_to_project() {
  say "Adding open issues to project #${PROJECT_NUMBER}"
  mapfile -t issue_urls < <(gh issue list --state open --limit 500 --json url -q '.[].url')
  for url in "${issue_urls[@]}"; do
    gh project item-add --owner "${OWNER}" "${PROJECT_NUMBER}" --url "$url" >/dev/null || true
  done
}

# === RUN ===
ensure_project

say "Creating milestones"
create_milestone "M0 — Planning & Infra"    "Scaffold, docs, CI guards, naming."
create_milestone "M1 — MVP"                  "Open/edit/save, status bar, prompts, safe writes."
create_milestone "M2 — QoL"                  "Search, undo/redo, wrap, line numbers, config, help."
create_milestone "M3 — Robustez & Reais"     "Encodings, autosave/swap, large files, long lines, perf."
create_milestone "M4 — Sintaxe & Temas"      "Light highlighting and themes."
create_milestone "M5 — Ergonomia"            "Clipboard, mouse, buffers, macros, hooks."

say "Creating labels"
create_label "type:bug"            d73a4a "Bug"
create_label "type:feature"        a2eeef "Feature"
create_label "type:docs"           0075ca "Documentation"
create_label "type:test"           0e8a16 "Testing"
create_label "area:core"           5319e7 "Core buffer & model"
create_label "area:ui"             c5def5 "Terminal UI & renderer"
create_label "area:io"             1d76db "File I/O & encodings"
create_label "area:config"         fbca04 "Config & keymaps"
create_label "area:search"         bfdadc "Search"
create_label "area:perf"           ff9a00 "Performance"
create_label "os:linux"            2b7489 "Linux specific"
create_label "os:macos"            a371f7 "macOS specific"
create_label "os:windows"          0e8a16 "Windows specific"
create_label "priority:high"       b60205 "High priority"
create_label "priority:medium"     fbca04 "Medium priority"
create_label "priority:low"        c2e0c6 "Low priority"
create_label "good first issue"    7057ff "Good onboarding task"
create_label "help wanted"         008672 "Looking for contributors"

say "Creating issues"

# M0 Planning & Infra
issue "Scaffold repo & initial docs" "M0 — Planning & Infra" "type:docs" \
"Add README, LICENSE, CONTRIBUTING, SECURITY, git attrs/ignore, editorconfig, CI guard, issue templates."
issue "Decide final project name" "M0 — Planning & Infra" "type:feature,area:ui" \
"Choose name (rnano/minim/folha/lima). Update README and badges."
issue "Define config format & locations" "M0 — Planning & Infra" "type:feature,area:config" \
"Decide TOML schema and per‑OS config dirs. Document precedence & defaults."
issue "Keymap policy (nano‑like + remap rules)" "M0 — Planning & Infra" "type:feature,area:config,area:ui" \
"List default keybindings and remapping constraints."
issue "CI guard & release targets" "M0 — Planning & Infra" "type:test" \
"Matrix os/toolchain; cargo steps gated on Cargo.toml availability."

# M1 MVP
issue "Load file: detect UTF‑8, CRLF/LF; binary guard" "M1 — MVP" "type:feature,area:io,priority:high" \
"Open path; detect encoding (UTF‑8 only for MVP), detect CRLF/LF; reject binary w/ clear message."
issue "Buffer core with rope; insert/delete/newline; tabs" "M1 — MVP" "type:feature,area:core,priority:high" \
"Rope storage; edit ops; configurable tab width; unit tests for edge cases."
issue "Viewport & cursor basics; scroll" "M1 — MVP" "type:feature,area:ui" \
"Render visible window; track cursor; vertical/horizontal scroll without wrap."
issue "Status bar (filename, line:col, modified)" "M1 — MVP" "type:feature,area:ui" \
"One‑line status with minimal redraw and update hooks."
issue "Prompts: write file name, goto line, confirm quit" "M1 — MVP" "type:feature,area:ui" \
"Inline prompt widget; ESC to cancel; validates input."
issue "Save: atomic write + backup ~ + preserve CRLF + perms" "M1 — MVP" "type:feature,area:io,priority:high" \
"Write to temp then rename; optional backup; keep EOL & file mode."
issue "Keybindings nano‑like (Ctrl+O/X/G/C/K/U/W/Y/V/A/E/_/J)" "M1 — MVP" "type:feature,area:config,area:ui" \
"Map keys to commands; display in help footer."
issue "Readonly mode + friendly errors" "M1 — MVP" "type:feature,area:io" \
"Open with --readonly; catch EPERM/ENOSPC and show actionable messages."
issue "Tests: buffer unit + property (basic)" "M1 — MVP" "type:test,area:core" \
"Unit tests for edits; property tests for insert/delete sequences."

# M2 QoL
issue "Incremental search + highlight + next/prev" "M2 — QoL" "type:feature,area:search,priority:high" \
"Literal and regex modes; case insensitive toggle; highlight matches."
issue "Undo/redo ring (configurable depth)" "M2 — QoL" "type:feature,area:core" \
"Command log with squash rules; tests for invariants."
issue "Soft wrap toggle; Unicode column widths" "M2 — QoL" "type:feature,area:ui" \
"Wrap at viewport width; respect grapheme width; keep cursor semantics."
issue "Line numbers margin (optional)" "M2 — QoL" "type:feature,area:ui" \
"Gutter with dynamic width; minimal redraw."
issue "Config file load (theme, tabsize, wrap, keymap)" "M2 — QoL" "type:feature,area:config" \
"Parse TOML and apply; doc default values."
issue "Help screen (Ctrl+G) with current keymap" "M2 — QoL" "type:feature,area:ui,type:docs" \
"Scrollable help view generated from keymap."

# M3 Robustez & Reais
issue "Encodings ISO‑8859‑1/Windows‑1252 (opt‑in)" "M3 — Robustez & Reais" "type:feature,area:io" \
"Decode/encode legacy encodings when forced via flag or config."
issue "Autosave/swap & crash recovery" "M3 — Robustez & Reais" "type:feature,area:io,priority:high" \
"Periodic snapshots; recover after crash; configurable interval."
issue "Large files: streaming load; avoid full reflow" "M3 — Robustez & Reais" "type:feature,area:perf" \
"Chunked read; lazy render; size thresholds documented."
issue "Very long lines: efficient hscroll" "M3 — Robustez & Reais" "type:feature,area:perf,area:ui" \
"Virtual columns; avoid O(n) per redraw on long lines."
issue "Cross‑platform EOL & permissions" "M3 — Robustez & Reais" "type:test,area:io" \
"Tests across OS; CRLF/LF parity; POSIX perms & Windows attrs."
issue "Perf marks & simple benchmarks" "M3 — Robustez & Reais" "type:test,area:perf" \
"Measure input→render latency; set 16ms target; track regressions."

# M4 Sintaxe & Temas
issue "Lightweight syntax rules per extension" "M4 — Sintaxe & Temas" "type:feature,area:ui" \
"INI/TOML/JSON/Markdown/Rust/SH; fast path; opt‑out by size."
issue "Themes (light/dark) selectable" "M4 — Sintaxe & Temas" "type:feature,area:ui,area:config" \
"Two built‑in themes; configurable in TOML."
issue "Auto‑disable highlight over N MB" "M4 — Sintaxe & Temas" "type:feature,area:perf" \
"Threshold setting; status bar notice when disabled."

# M5 Ergonomia
issue "Clipboard integration with OS" "M5 — Ergonomia" "type:feature,area:ui" \
"Use OS clipboard when available; TUI fallback."
issue "Optional mouse for selection/scroll" "M5 — Ergonomia" "type:feature,area:ui" \
"Enable/disable via config; avoid perf hits."
issue "Multiple buffers + quick switcher" "M5 — Ergonomia" "type:feature,area:core,area:ui" \
"Open multiple files and switch; simple buffer list."
issue "Simple macros (record/play in session)" "M5 — Ergonomia" "type:feature,area:ui" \
"Record keystrokes; in‑memory; per‑session only."
issue "On‑save hooks (e.g., rustfmt)" "M5 — Ergonomia" "type:feature,area:io" \
"Run external commands on save (opt‑in, off by default)."

add_all_issues_to_project

say "Done. Project #${PROJECT_NUMBER} populated with open issues."
```bash
#!/usr/bin/env bash
set -euo pipefail

# Usage: run from repository root after pushing to GitHub
# Requires: gh (https://cli.github.com/) authenticated

# Detect owner/repo from git remote
REPO=${REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
if [[ -z "${REPO}" ]]; then
  echo "Cannot detect repo. Set REPO=owner/name and retry." >&2
  exit 1
fi

say() { printf "\033[1;36m==> %s\033[0m\n" "$*"; }

say "Using repo: ${REPO}"

# --- Milestones ---
create_milestone() {
  local title="$1"; shift
  local description="$*"
  gh api --silent \
    --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/milestones" \
    -f title="$title" -f state=open -f description="$description" || true
}

say "Creating milestones"
create_milestone "M0 — Planning & Infra"    "Scaffold, docs, CI guards, naming."
create_milestone "M1 — MVP"                  "Open/edit/save, status bar, prompts, safe writes."
create_milestone "M2 — QoL"                  "Search, undo/redo, wrap, line numbers, config, help."
create_milestone "M3 — Robustez & Reais"     "Encodings, autosave/swap, large files, long lines, perf."
create_milestone "M4 — Sintaxe & Temas"      "Light highlighting and themes."
create_milestone "M5 — Ergonomia"            "Clipboard, mouse, buffers, macros, hooks."

# --- Labels ---
create_label() {
  local name="$1" color="$2" description="$3"
  gh api --silent --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/labels" \
    -f name="$name" -f color="$color" -f description="$description" || true
}

say "Creating labels"
create_label "type:bug"            d73a4a "Bug"
create_label "type:feature"        a2eeef "Feature"
create_label "type:docs"           0075ca "Documentation"
create_label "type:test"           0e8a16 "Testing"
create_label "area:core"           5319e7 "Core buffer & model"
create_label "area:ui"             c5def5 "Terminal UI & renderer"
create_label "area:io"             1d76db "File I/O & encodings"
create_label "area:config"         fbca04 "Config & keymaps"
create_label "area:search"         bfdadc "Search"
create_label "area:perf"           ff9a00 "Performance"
create_label "os:linux"            2b7489 "Linux specific"
create_label "os:macos"            a371f7 "macOS specific"
create_label "os:windows"          0e8a16 "Windows specific"
create_label "priority:high"       b60205 "High priority"
create_label "priority:medium"     fbca04 "Medium priority"
create_label "priority:low"        c2e0c6 "Low priority"
create_label "good first issue"    7057ff "Good onboarding task"
create_label "help wanted"         008672 "Looking for contributors"

# --- Issues ---
issue() {
  local title="$1" milestone="$2" labels="$3" body="$4"
  gh issue create --repo "$REPO" \
    --title "$title" \
    --milestone "$milestone" \
    --label $(echo "$labels" | sed 's/,/ --label /g') \
    --body "$body" >/dev/null
}

say "Creating issues"

# M0 Planning & Infra
issue "Scaffold repo & initial docs" "M0 — Planning & Infra" "type:docs" \
"Add README, LICENSE, CONTRIBUTING, SECURITY, git attrs/ignore, editorconfig, CI guard, issue templates."
issue "Decide final project name" "M0 — Planning & Infra" "type:feature,area:ui" \
"Choose name (rnano/minim/folha/lima). Update README and badges."
issue "Define config format & locations" "M0 — Planning & Infra" "type:feature,area:config" \
"Decide TOML schema and per‑OS config dirs. Document precedence & defaults."
issue "Keymap policy (nano‑like + remap rules)" "M0 — Planning & Infra" "type:feature,area:config,area:ui" \
"List default keybindings and remapping constraints."
issue "CI guard & release targets" "M0 — Planning & Infra" "type:test" \
"Matrix os/toolchain; cargo steps gated on Cargo.toml availability."

# M1 MVP
issue "Load file: detect UTF‑8, CRLF/LF; binary guard" "M1 — MVP" "type:feature,area:io,priority:high" \
"Open path; detect encoding (UTF‑8 only for MVP), detect CRLF/LF; reject binary w/ clear message."
issue "Buffer core with rope; insert/delete/newline; tabs" "M1 — MVP" "type:feature,area:core,priority:high" \
"Rope storage; edit ops; configurable tab width; unit tests for edge cases."
issue "Viewport & cursor basics; scroll" "M1 — MVP" "type:feature,area:ui" \
"Render visible window; track cursor; vertical/horizontal scroll without wrap."
issue "Status bar (filename, line:col, modified)" "M1 — MVP" "type:feature,area:ui" \
"One‑line status with minimal redraw and update hooks."
issue "Prompts: write file name, goto line, confirm quit" "M1 — MVP" "type:feature,area:ui" \
"Inline prompt widget; ESC to cancel; validates input."
issue "Save: atomic write + backup ~ + preserve CRLF + perms" "M1 — MVP" "type:feature,area:io,priority:high" \
"Write to temp then rename; optional backup; keep EOL & file mode."
issue "Keybindings nano‑like (Ctrl+O/X/G/C/K/U/W/Y/V/A/E/_/J)" "M1 — MVP" "type:feature,area:config,area:ui" \
"Map keys to commands; display in help footer."
issue "Readonly mode + friendly errors" "M1 — MVP" "type:feature,area:io" \
"Open with --readonly; catch EPERM/ENOSPC and show actionable messages."
issue "Tests: buffer unit + property (basic)" "M1 — MVP" "type:test,area:core" \
"Unit tests for edits; property tests for insert/delete sequences."

# M2 QoL
issue "Incremental search + highlight + next/prev" "M2 — QoL" "type:feature,area:search,priority:high" \
"Literal and regex modes; case insensitive toggle; highlight matches."
issue "Undo/redo ring (configurable depth)" "M2 — QoL" "type:feature,area:core" \
"Command log with squash rules; tests for invariants."
issue "Soft wrap toggle; Unicode column widths" "M2 — QoL" "type:feature,area:ui" \
"Wrap at viewport width; respect grapheme width; keep cursor semantics."
issue "Line numbers margin (optional)" "M2 — QoL" "type:feature,area:ui" \
"Gutter with dynamic width; minimal redraw."
issue "Config file load (theme, tabsize, wrap, keymap)" "M2 — QoL" "type:feature,area:config" \
"Parse TOML and apply; doc default values."
issue "Help screen (Ctrl+G) with current keymap" "M2 — QoL" "type:feature,area:ui,type:docs" \
"Scrollable help view generated from keymap."

# M3 Robustez & Reais
issue "Encodings ISO‑8859‑1/Windows‑1252 (opt‑in)" "M3 — Robustez & Reais" "type:feature,area:io" \
"Decode/encode legacy encodings when forced via flag or config."
issue "Autosave/swap & crash recovery" "M3 — Robustez & Reais" "type:feature,area:io,priority:high" \
"Periodic snapshots; recover after crash; configurable interval."
issue "Large files: streaming load; avoid full reflow" "M3 — Robustez & Reais" "type:feature,area:perf" \
"Chunked read; lazy render; size thresholds documented."
issue "Very long lines: efficient hscroll" "M3 — Robustez & Reais" "type:feature,area:perf,area:ui" \
"Virtual columns; avoid O(n) per redraw on long lines."
issue "Cross‑platform EOL & permissions" "M3 — Robustez & Reais" "type:test,area:io" \
"Tests across OS; CRLF/LF parity; POSIX perms & Windows attrs."
issue "Perf marks & simple benchmarks" "M3 — Robustez & Reais" "type:test,area:perf" \
"Measure input→render latency; set 16ms target; track regressions."

# M4 Sintaxe & Temas
issue "Lightweight syntax rules per extension" "M4 — Sintaxe & Temas" "type:feature,area:ui" \
"INI/TOML/JSON/Markdown/Rust/SH; fast path; opt‑out by size."
issue "Themes (light/dark) selectable" "M4 — Sintaxe & Temas" "type:feature,area:ui,area:config" \
"Two built‑in themes; configurable in TOML."
issue "Auto‑disable highlight over N MB" "M4 — Sintaxe & Temas" "type:feature,area:perf" \
"Threshold setting; status bar notice when disabled."

# M5 Ergonomia
issue "Clipboard integration with OS" "M5 — Ergonomia" "type:feature,area:ui" \
"Use OS clipboard when available; TUI fallback."
issue "Optional mouse for selection/scroll" "M5 — Ergonomia" "type:feature,area:ui" \
"Enable/disable via config; avoid perf hits."
issue "Multiple buffers + quick switcher" "M5 — Ergonomia" "type:feature,area:core,area:ui" \
"Open multiple files and switch; simple buffer list."
issue "Simple macros (record/play in session)" "M5 — Ergonomia" "type:feature,area:ui" \
"Record keystrokes; in‑memory; per‑session only."
issue "On‑save hooks (e.g., rustfmt)" "M5 — Ergonomia" "type:feature,area:io" \
"Run external commands on save (opt‑in, off by default)."

say "Done."#!/usr/bin/env bash
set -euo pipefail

# Usage: run from repository root after pushing to GitHub
# Requires: gh (https://cli.github.com/) authenticated
# Also requires: jq

# Detect owner/repo from git remote
REPO=${REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
if [[ -z "${REPO}" ]]; then
  echo "Cannot detect repo. Set REPO=owner/name and retry." >&2
  exit 1
fi
OWNER=${OWNER:-${REPO%%/*}}

say() { printf "[1;36m==> %s[0m
" "$*"; }

say "Using repo: ${REPO} (owner: ${OWNER})"

# --- (1) Create Project (if missing) ---
PROJECT_TITLE=${PROJECT_TITLE:-"rnano Roadmap"}
PROJECT_NUMBER=${PROJECT_NUMBER:-}

ensure_project() {
  if [[ -n "${PROJECT_NUMBER}" ]]; then
    say "Using provided project number: ${PROJECT_NUMBER}"
    return
  fi
  say "Ensuring GitHub Project exists: '${PROJECT_TITLE}'"
  # Try to find existing by title
  existing=$(gh project list --owner "${OWNER}" --format json | jq -r --arg t "${PROJECT_TITLE}" '.projects[] | select(.title==$t) | .number' || true)
  if [[ -n "${existing}" ]]; then
    PROJECT_NUMBER="$existing"
    say "Found existing project #${PROJECT_NUMBER}"
    return
  fi
  # Create new project and capture number
  PROJECT_NUMBER=$(gh project create --owner "${OWNER}" --title "${PROJECT_TITLE}" --format json | jq -r '.number')
  say "Created project #${PROJECT_NUMBER} — switch its view to Board in the GitHub UI if you prefer"
}

# --- (2) Milestones ---
create_milestone() {
  local title="$1"; shift
  local description="$*"
  gh api --silent \
    --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/milestones" \
    -f title="$title" -f state=open -f description="$description" || true
}

# --- (3) Labels ---
create_label() {
  local name="$1" color="$2" description="$3"
  gh api --silent --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/labels" \
    -f name="$name" -f color="$color" -f description="$description" || true
}

# --- (4) Issues ---
# Helper to create an issue attached to a milestone and with labels
issue() {
  local title="$1" milestone="$2" labels="$3" body="$4"
  gh issue create --repo "$REPO" \
    --title "$title" \
    --milestone "$milestone" \
    --label $(echo "$labels" | sed 's/,/ --label /g') \
    --body "$body" >/dev/null
}

# --- (5) Add all repo issues to Project ---
add_all_issues_to_project() {
  say "Adding open issues to project #${PROJECT_NUMBER}"
  mapfile -t issue_urls < <(gh issue list --state open --limit 500 --json url -q '.[].url')
  for url in "${issue_urls[@]}"; do
    gh project item-add --owner "${OWNER}" --project "${PROJECT_NUMBER}" --url "$url" >/dev/null || true
  done
}

# === RUN ===
ensure_project

say "Creating milestones"
create_milestone "M0 — Planning & Infra"    "Scaffold, docs, CI guards, naming."
create_milestone "M1 — MVP"                  "Open/edit/save, status bar, prompts, safe writes."
create_milestone "M2 — QoL"                  "Search, undo/redo, wrap, line numbers, config, help."
create_milestone "M3 — Robustez & Reais"     "Encodings, autosave/swap, large files, long lines, perf."
create_milestone "M4 — Sintaxe & Temas"      "Light highlighting and themes."
create_milestone "M5 — Ergonomia"            "Clipboard, mouse, buffers, macros, hooks."

say "Creating labels"
create_label "type:bug"            d73a4a "Bug"
create_label "type:feature"        a2eeef "Feature"
create_label "type:docs"           0075ca "Documentation"
create_label "type:test"           0e8a16 "Testing"
create_label "area:core"           5319e7 "Core buffer & model"
create_label "area:ui"             c5def5 "Terminal UI & renderer"
create_label "area:io"             1d76db "File I/O & encodings"
create_label "area:config"         fbca04 "Config & keymaps"
create_label "area:search"         bfdadc "Search"
create_label "area:perf"           ff9a00 "Performance"
create_label "os:linux"            2b7489 "Linux specific"
create_label "os:macos"            a371f7 "macOS specific"
create_label "os:windows"          0e8a16 "Windows specific"
create_label "priority:high"       b60205 "High priority"
create_label "priority:medium"     fbca04 "Medium priority"
create_label "priority:low"        c2e0c6 "Low priority"
create_label "good first issue"    7057ff "Good onboarding task"
create_label "help wanted"         008672 "Looking for contributors"

say "Creating issues"

# M0 Planning & Infra
issue "Scaffold repo & initial docs" "M0 — Planning & Infra" "type:docs" \
"Add README, LICENSE, CONTRIBUTING, SECURITY, git attrs/ignore, editorconfig, CI guard, issue templates."
issue "Decide final project name" "M0 — Planning & Infra" "type:feature,area:ui" \
"Choose name (rnano/minim/folha/lima). Update README and badges."
issue "Define config format & locations" "M0 — Planning & Infra" "type:feature,area:config" \
"Decide TOML schema and per‑OS config dirs. Document precedence & defaults."
issue "Keymap policy (nano‑like + remap rules)" "M0 — Planning & Infra" "type:feature,area:config,area:ui" \
"List default keybindings and remapping constraints."
issue "CI guard & release targets" "M0 — Planning & Infra" "type:test" \
"Matrix os/toolchain; cargo steps gated on Cargo.toml availability."

# M1 MVP
issue "Load file: detect UTF‑8, CRLF/LF; binary guard" "M1 — MVP" "type:feature,area:io,priority:high" \
"Open path; detect encoding (UTF‑8 only for MVP), detect CRLF/LF; reject binary w/ clear message."
issue "Buffer core with rope; insert/delete/newline; tabs" "M1 — MVP" "type:feature,area:core,priority:high" \
"Rope storage; edit ops; configurable tab width; unit tests for edge cases."
issue "Viewport & cursor basics; scroll" "M1 — MVP" "type:feature,area:ui" \
"Render visible window; track cursor; vertical/horizontal scroll without wrap."
issue "Status bar (filename, line:col, modified)" "M1 — MVP" "type:feature,area:ui" \
"One‑line status with minimal redraw and update hooks."
issue "Prompts: write file name, goto line, confirm quit" "M1 — MVP" "type:feature,area:ui" \
"Inline prompt widget; ESC to cancel; validates input."
issue "Save: atomic write + backup ~ + preserve CRLF + perms" "M1 — MVP" "type:feature,area:io,priority:high" \
"Write to temp then rename; optional backup; keep EOL & file mode."
issue "Keybindings nano‑like (Ctrl+O/X/G/C/K/U/W/Y/V/A/E/_/J)" "M1 — MVP" "type:feature,area:config,area:ui" \
"Map keys to commands; display in help footer."
issue "Readonly mode + friendly errors" "M1 — MVP" "type:feature,area:io" \
"Open with --readonly; catch EPERM/ENOSPC and show actionable messages."
issue "Tests: buffer unit + property (basic)" "M1 — MVP" "type:test,area:core" \
"Unit tests for edits; property tests for insert/delete sequences."

# M2 QoL
issue "Incremental search + highlight + next/prev" "M2 — QoL" "type:feature,area:search,priority:high" \
"Literal and regex modes; case insensitive toggle; highlight matches."
issue "Undo/redo ring (configurable depth)" "M2 — QoL" "type:feature,area:core" \
"Command log with squash rules; tests for invariants."
issue "Soft wrap toggle; Unicode column widths" "M2 — QoL" "type:feature,area:ui" \
"Wrap at viewport width; respect grapheme width; keep cursor semantics."
issue "Line numbers margin (optional)" "M2 — QoL" "type:feature,area:ui" \
"Gutter with dynamic width; minimal redraw."
issue "Config file load (theme, tabsize, wrap, keymap)" "M2 — QoL" "type:feature,area:config" \
"Parse TOML and apply; doc default values."
issue "Help screen (Ctrl+G) with current keymap" "M2 — QoL" "type:feature,area:ui,type:docs" \
"Scrollable help view generated from keymap."

# M3 Robustez & Reais
issue "Encodings ISO‑8859‑1/Windows‑1252 (opt‑in)" "M3 — Robustez & Reais" "type:feature,area:io" \
"Decode/encode legacy encodings when forced via flag or config."
issue "Autosave/swap & crash recovery" "M3 — Robustez & Reais" "type:feature,area:io,priority:high" \
"Periodic snapshots; recover after crash; configurable interval."
issue "Large files: streaming load; avoid full reflow" "M3 — Robustez & Reais" "type:feature,area:perf" \
"Chunked read; lazy render; size thresholds documented."
issue "Very long lines: efficient hscroll" "M3 — Robustez & Reais" "type:feature,area:perf,area:ui" \
"Virtual columns; avoid O(n) per redraw on long lines."
issue "Cross‑platform EOL & permissions" "M3 — Robustez & Reais" "type:test,area:io" \
"Tests across OS; CRLF/LF parity; POSIX perms & Windows attrs."
issue "Perf marks & simple benchmarks" "M3 — Robustez & Reais" "type:test,area:perf" \
"Measure input→render latency; set 16ms target; track regressions."

# M4 Sintaxe & Temas
issue "Lightweight syntax rules per extension" "M4 — Sintaxe & Temas" "type:feature,area:ui" \
"INI/TOML/JSON/Markdown/Rust/SH; fast path; opt‑out by size."
issue "Themes (light/dark) selectable" "M4 — Sintaxe & Temas" "type:feature,area:ui,area:config" \
"Two built‑in themes; configurable in TOML."
issue "Auto‑disable highlight over N MB" "M4 — Sintaxe & Temas" "type:feature,area:perf" \
"Threshold setting; status bar notice when disabled."

# M5 Ergonomia
issue "Clipboard integration with OS" "M5 — Ergonomia" "type:feature,area:ui" \
"Use OS clipboard when available; TUI fallback."
issue "Optional mouse for selection/scroll" "M5 — Ergonomia" "type:feature,area:ui" \
"Enable/disable via config; avoid perf hits."
issue "Multiple buffers + quick switcher" "M5 — Ergonomia" "type:feature,area:core,area:ui" \
"Open multiple files and switch; simple buffer list."
issue "Simple macros (record/play in session)" "M5 — Ergonomia" "type:feature,area:ui" \
"Record keystrokes; in‑memory; per‑session only."
issue "On‑save hooks (e.g., rustfmt)" "M5 — Ergonomia" "type:feature,area:io" \
"Run external commands on save (opt‑in, off by default)."

add_all_issues_to_project

say "Done. Project #${PROJECT_NUMBER} populated with open issues."
```bash
#!/usr/bin/env bash
set -euo pipefail

# Usage: run from repository root after pushing to GitHub
# Requires: gh (https://cli.github.com/) authenticated

# Detect owner/repo from git remote
REPO=${REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
if [[ -z "${REPO}" ]]; then
  echo "Cannot detect repo. Set REPO=owner/name and retry." >&2
  exit 1
fi

say() { printf "\033[1;36m==> %s\033[0m\n" "$*"; }

say "Using repo: ${REPO}"

# --- Milestones ---
create_milestone() {
  local title="$1"; shift
  local description="$*"
  gh api --silent \
    --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/milestones" \
    -f title="$title" -f state=open -f description="$description" || true
}

say "Creating milestones"
create_milestone "M0 — Planning & Infra"    "Scaffold, docs, CI guards, naming."
create_milestone "M1 — MVP"                  "Open/edit/save, status bar, prompts, safe writes."
create_milestone "M2 — QoL"                  "Search, undo/redo, wrap, line numbers, config, help."
create_milestone "M3 — Robustez & Reais"     "Encodings, autosave/swap, large files, long lines, perf."
create_milestone "M4 — Sintaxe & Temas"      "Light highlighting and themes."
create_milestone "M5 — Ergonomia"            "Clipboard, mouse, buffers, macros, hooks."

# --- Labels ---
create_label() {
  local name="$1" color="$2" description="$3"
  gh api --silent --method POST \
    -H "Accept: application/vnd.github+json" \
    "/repos/${REPO}/labels" \
    -f name="$name" -f color="$color" -f description="$description" || true
}

say "Creating labels"
create_label "type:bug"            d73a4a "Bug"
create_label "type:feature"        a2eeef "Feature"
create_label "type:docs"           0075ca "Documentation"
create_label "type:test"           0e8a16 "Testing"
create_label "area:core"           5319e7 "Core buffer & model"
create_label "area:ui"             c5def5 "Terminal UI & renderer"
create_label "area:io"             1d76db "File I/O & encodings"
create_label "area:config"         fbca04 "Config & keymaps"
create_label "area:search"         bfdadc "Search"
create_label "area:perf"           ff9a00 "Performance"
create_label "os:linux"            2b7489 "Linux specific"
create_label "os:macos"            a371f7 "macOS specific"
create_label "os:windows"          0e8a16 "Windows specific"
create_label "priority:high"       b60205 "High priority"
create_label "priority:medium"     fbca04 "Medium priority"
create_label "priority:low"        c2e0c6 "Low priority"
create_label "good first issue"    7057ff "Good onboarding task"
create_label "help wanted"         008672 "Looking for contributors"

# --- Issues ---
issue() {
  local title="$1" milestone="$2" labels="$3" body="$4"
  gh issue create --repo "$REPO" \
    --title "$title" \
    --milestone "$milestone" \
    --label $(echo "$labels" | sed 's/,/ --label /g') \
    --body "$body" >/dev/null
}

say "Creating issues"

# M0 Planning & Infra
issue "Scaffold repo & initial docs" "M0 — Planning & Infra" "type:docs" \
"Add README, LICENSE, CONTRIBUTING, SECURITY, git attrs/ignore, editorconfig, CI guard, issue templates."
issue "Decide final project name" "M0 — Planning & Infra" "type:feature,area:ui" \
"Choose name (rnano/minim/folha/lima). Update README and badges."
issue "Define config format & locations" "M0 — Planning & Infra" "type:feature,area:config" \
"Decide TOML schema and per‑OS config dirs. Document precedence & defaults."
issue "Keymap policy (nano‑like + remap rules)" "M0 — Planning & Infra" "type:feature,area:config,area:ui" \
"List default keybindings and remapping constraints."
issue "CI guard & release targets" "M0 — Planning & Infra" "type:test" \
"Matrix os/toolchain; cargo steps gated on Cargo.toml availability."

# M1 MVP
issue "Load file: detect UTF‑8, CRLF/LF; binary guard" "M1 — MVP" "type:feature,area:io,priority:high" \
"Open path; detect encoding (UTF‑8 only for MVP), detect CRLF/LF; reject binary w/ clear message."
issue "Buffer core with rope; insert/delete/newline; tabs" "M1 — MVP" "type:feature,area:core,priority:high" \
"Rope storage; edit ops; configurable tab width; unit tests for edge cases."
issue "Viewport & cursor basics; scroll" "M1 — MVP" "type:feature,area:ui" \
"Render visible window; track cursor; vertical/horizontal scroll without wrap."
issue "Status bar (filename, line:col, modified)" "M1 — MVP" "type:feature,area:ui" \
"One‑line status with minimal redraw and update hooks."
issue "Prompts: write file name, goto line, confirm quit" "M1 — MVP" "type:feature,area:ui" \
"Inline prompt widget; ESC to cancel; validates input."
issue "Save: atomic write + backup ~ + preserve CRLF + perms" "M1 — MVP" "type:feature,area:io,priority:high" \
"Write to temp then rename; optional backup; keep EOL & file mode."
issue "Keybindings nano‑like (Ctrl+O/X/G/C/K/U/W/Y/V/A/E/_/J)" "M1 — MVP" "type:feature,area:config,area:ui" \
"Map keys to commands; display in help footer."
issue "Readonly mode + friendly errors" "M1 — MVP" "type:feature,area:io" \
"Open with --readonly; catch EPERM/ENOSPC and show actionable messages."
issue "Tests: buffer unit + property (basic)" "M1 — MVP" "type:test,area:core" \
"Unit tests for edits; property tests for insert/delete sequences."

# M2 QoL
issue "Incremental search + highlight + next/prev" "M2 — QoL" "type:feature,area:search,priority:high" \
"Literal and regex modes; case insensitive toggle; highlight matches."
issue "Undo/redo ring (configurable depth)" "M2 — QoL" "type:feature,area:core" \
"Command log with squash rules; tests for invariants."
issue "Soft wrap toggle; Unicode column widths" "M2 — QoL" "type:feature,area:ui" \
"Wrap at viewport width; respect grapheme width; keep cursor semantics."
issue "Line numbers margin (optional)" "M2 — QoL" "type:feature,area:ui" \
"Gutter with dynamic width; minimal redraw."
issue "Config file load (theme, tabsize, wrap, keymap)" "M2 — QoL" "type:feature,area:config" \
"Parse TOML and apply; doc default values."
issue "Help screen (Ctrl+G) with current keymap" "M2 — QoL" "type:feature,area:ui,type:docs" \
"Scrollable help view generated from keymap."

# M3 Robustez & Reais
issue "Encodings ISO‑8859‑1/Windows‑1252 (opt‑in)" "M3 — Robustez & Reais" "type:feature,area:io" \
"Decode/encode legacy encodings when forced via flag or config."
issue "Autosave/swap & crash recovery" "M3 — Robustez & Reais" "type:feature,area:io,priority:high" \
"Periodic snapshots; recover after crash; configurable interval."
issue "Large files: streaming load; avoid full reflow" "M3 — Robustez & Reais" "type:feature,area:perf" \
"Chunked read; lazy render; size thresholds documented."
issue "Very long lines: efficient hscroll" "M3 — Robustez & Reais" "type:feature,area:perf,area:ui" \
"Virtual columns; avoid O(n) per redraw on long lines."
issue "Cross‑platform EOL & permissions" "M3 — Robustez & Reais" "type:test,area:io" \
"Tests across OS; CRLF/LF parity; POSIX perms & Windows attrs."
issue "Perf marks & simple benchmarks" "M3 — Robustez & Reais" "type:test,area:perf" \
"Measure input→render latency; set 16ms target; track regressions."

# M4 Sintaxe & Temas
issue "Lightweight syntax rules per extension" "M4 — Sintaxe & Temas" "type:feature,area:ui" \
"INI/TOML/JSON/Markdown/Rust/SH; fast path; opt‑out by size."
issue "Themes (light/dark) selectable" "M4 — Sintaxe & Temas" "type:feature,area:ui,area:config" \
"Two built‑in themes; configurable in TOML."
issue "Auto‑disable highlight over N MB" "M4 — Sintaxe & Temas" "type:feature,area:perf" \
"Threshold setting; status bar notice when disabled."

# M5 Ergonomia
issue "Clipboard integration with OS" "M5 — Ergonomia" "type:feature,area:ui" \
"Use OS clipboard when available; TUI fallback."
issue "Optional mouse for selection/scroll" "M5 — Ergonomia" "type:feature,area:ui" \
"Enable/disable via config; avoid perf hits."
issue "Multiple buffers + quick switcher" "M5 — Ergonomia" "type:feature,area:core,area:ui" \
"Open multiple files and switch; simple buffer list."
issue "Simple macros (record/play in session)" "M5 — Ergonomia" "type:feature,area:ui" \
"Record keystrokes; in‑memory; per‑session only."
issue "On‑save hooks (e.g., rustfmt)" "M5 — Ergonomia" "type:feature,area:io" \
"Run external commands on save (opt‑in, off by default)."

say "Done."
