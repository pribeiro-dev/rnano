//! User configuration: schema, platform paths, and a minimal parser.
//!
//! **Config file location** (first one found wins):
//!
//! | Platform | Path |
//! |----------|------|
//! | Linux / macOS | `$XDG_CONFIG_HOME/rnano/config.toml` |
//! | Linux / macOS (fallback) | `$HOME/.config/rnano/config.toml` |
//! | Windows | `%APPDATA%\rnano\config.toml` |
//!
//! **Precedence** (lowest → highest): built-in defaults → config file → CLI flags.
//!
//! **Format**: a strict subset of TOML — top-level `key = value` pairs only.
//! `#` starts a line comment. `[section]` headers are accepted and ignored.
//! Unknown keys are silently skipped so older binaries tolerate new options.
//!
//! **Example**:
//! ```toml
//! tab_width        = 4      # display columns per tab stop  (default: 4)
//! line_numbers     = false  # show line-number gutter        (default: false)
//! soft_wrap        = false  # wrap long lines at viewport     (default: false)
//! backup           = false  # write <file>~ before saving    (default: false)
//! undo_depth       = 1000   # max undo history entries       (default: 1000)
//! highlight_max_kb = 512    # disable syntax hl above N KiB  (default: 512)
//! ```

use std::path::PathBuf;

/// Runtime configuration for rnano.
///
/// Build with [`Config::default`], then optionally overlay a file via
/// [`Config::load`] and apply CLI flags via [`Config::apply_cli`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Display columns per tab stop.
    pub tab_width: usize,
    /// Show line-number gutter.
    pub line_numbers: bool,
    /// Soft-wrap long lines at the viewport width.
    pub soft_wrap: bool,
    /// Write a `<file>~` backup before overwriting.
    pub backup: bool,
    /// Maximum number of undo history entries.
    pub undo_depth: usize,
    /// Disable syntax highlighting for files larger than this many KiB.
    pub highlight_max_kb: u64,
    /// Autosave interval in seconds (0 = disabled).
    pub autosave_interval: u64,
    /// Colour theme: `"dark"` (default) or `"light"`.
    pub theme: String,
    /// Enable syntax highlighting.
    pub syntax: bool,
    /// Shell command run after a successful save; `%f` → file path.  Empty = disabled.
    pub on_save_hook: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tab_width: 4,
            line_numbers: false,
            soft_wrap: false,
            backup: false,
            undo_depth: 1000,
            highlight_max_kb: 512,
            autosave_interval: 0,
            theme: "dark".to_string(),
            syntax: true,
            on_save_hook: String::new(),
        }
    }
}

impl Config {
    /// Load from the platform config file, falling back to defaults if the
    /// file is absent.  Returns an error string on parse failure.
    pub fn load() -> Result<Config, String> {
        let Some(path) = config_path() else {
            return Ok(Config::default());
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Apply CLI flag overrides on top of file-loaded values.
    /// CLI flags only set values to `true`; they never force `false`.
    pub fn apply_cli(&mut self, backup: bool) {
        if backup {
            self.backup = true;
        }
    }
}

/// Returns the platform config file path without checking for existence.
///
/// Useful for printing the expected path in `--help` output.
pub fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("rnano").join("config.toml"))
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg).join("rnano").join("config.toml"));
        }
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".config")
                .join("rnano")
                .join("config.toml")
        })
    }
}

/// Parse a minimal TOML subset into a [`Config`].
///
/// Accepts: `key = value`, blank lines, and `# comments`.
/// `[section]` headers are skipped. String values may be optionally quoted.
/// Unknown keys are ignored. Tab characters are allowed around `=`.
fn parse(text: &str) -> Result<Config, String> {
    let mut cfg = Config::default();
    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let (key, val) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected `key = value`", lineno + 1))?;
        let key = key.trim();
        let val = val.trim().trim_matches('"');
        apply_key(&mut cfg, key, val, lineno + 1)?;
    }
    Ok(cfg)
}

fn strip_comment(s: &str) -> &str {
    // A `#` only starts a comment when it is not inside a quoted string.
    // Our values are either bare integers/booleans or optionally double-quoted
    // strings, so we only need to skip `#` outside of `"…"`.
    let mut in_quotes = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &s[..i],
            _ => {}
        }
    }
    s
}

fn apply_key(cfg: &mut Config, key: &str, val: &str, line: usize) -> Result<(), String> {
    match key {
        "tab_width" => {
            cfg.tab_width = val
                .parse::<usize>()
                .map_err(|_| format!("line {line}: `tab_width` must be a positive integer"))?
                .max(1);
        }
        "line_numbers" => cfg.line_numbers = parse_bool(val, line)?,
        "soft_wrap" => cfg.soft_wrap = parse_bool(val, line)?,
        "backup" => cfg.backup = parse_bool(val, line)?,
        "undo_depth" => {
            cfg.undo_depth = val
                .parse::<usize>()
                .map_err(|_| format!("line {line}: `undo_depth` must be a positive integer"))?
                .max(1);
        }
        "highlight_max_kb" => {
            cfg.highlight_max_kb = val.parse::<u64>().map_err(|_| {
                format!("line {line}: `highlight_max_kb` must be a non-negative integer")
            })?;
        }
        "autosave_interval" => {
            cfg.autosave_interval = val.parse::<u64>().map_err(|_| {
                format!("line {line}: `autosave_interval` must be a non-negative integer")
            })?;
        }
        "theme" => cfg.theme = val.to_string(),
        "syntax" => cfg.syntax = parse_bool(val, line)?,
        "on_save_hook" => cfg.on_save_hook = val.to_string(),
        _ => {} // forward-compatible: unknown keys are silently skipped
    }
    Ok(())
}

fn parse_bool(val: &str, line: usize) -> Result<bool, String> {
    match val {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!(
            "line {line}: expected `true` or `false`, got `{other}`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.tab_width, 4);
        assert!(!cfg.line_numbers);
        assert!(!cfg.soft_wrap);
        assert!(!cfg.backup);
        assert_eq!(cfg.undo_depth, 1000);
        assert_eq!(cfg.highlight_max_kb, 512);
    }

    #[test]
    fn parse_all_keys() {
        let text = "
# rnano config
tab_width        = 2
line_numbers     = true
soft_wrap        = true
backup           = true
undo_depth       = 500
highlight_max_kb = 256
";
        let cfg = parse(text).unwrap();
        assert_eq!(cfg.tab_width, 2);
        assert!(cfg.line_numbers);
        assert!(cfg.soft_wrap);
        assert!(cfg.backup);
        assert_eq!(cfg.undo_depth, 500);
        assert_eq!(cfg.highlight_max_kb, 256);
    }

    #[test]
    fn parse_blank_lines_and_comments() {
        let cfg = parse("\n# comment\n\ntab_width = 8\n").unwrap();
        assert_eq!(cfg.tab_width, 8);
    }

    #[test]
    fn parse_quoted_values() {
        // Quoted strings are valid TOML for booleans when the user makes a mistake;
        // we strip the quotes and re-parse.
        let cfg = parse("backup = \"true\"\n").unwrap();
        assert!(cfg.backup);
    }

    #[test]
    fn parse_inline_comment() {
        let cfg = parse("tab_width = 2 # two spaces\n").unwrap();
        assert_eq!(cfg.tab_width, 2);
    }

    #[test]
    fn parse_section_header_ignored() {
        let cfg = parse("[editor]\ntab_width = 3\n").unwrap();
        assert_eq!(cfg.tab_width, 3);
    }

    #[test]
    fn parse_unknown_key_ignored() {
        let cfg = parse("future_feature = 42\ntab_width = 3\n").unwrap();
        assert_eq!(cfg.tab_width, 3);
    }

    #[test]
    fn parse_bad_bool_errors() {
        assert!(parse("line_numbers = yes\n").is_err());
        assert!(parse("backup = 1\n").is_err());
    }

    #[test]
    fn parse_bad_int_errors() {
        assert!(parse("tab_width = abc\n").is_err());
    }

    #[test]
    fn tab_width_clamped_to_one() {
        let cfg = parse("tab_width = 0\n").unwrap();
        assert_eq!(cfg.tab_width, 1);
    }

    #[test]
    fn apply_cli_backup_forces_true() {
        let mut cfg = Config::default();
        assert!(!cfg.backup);
        cfg.apply_cli(true);
        assert!(cfg.backup);
    }

    #[test]
    fn apply_cli_false_does_not_override_file() {
        let mut cfg = Config {
            backup: true,
            ..Config::default()
        };
        cfg.apply_cli(false); // --backup not passed
        assert!(cfg.backup); // still true from config file
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        // Point XDG_CONFIG_HOME at a nonexistent dir so load() finds nothing.
        // SAFETY: single-threaded test; no other thread reads this var concurrently.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/nonexistent_rnano_test_dir") };
        let cfg = Config::load().unwrap();
        assert_eq!(cfg, Config::default());
    }
}
