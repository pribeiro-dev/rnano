mod buffer;
mod clipboard;
mod config;
mod editor;
mod highlight;
mod history;

use std::path::PathBuf;
use std::process::ExitCode;

use buffer::Buffer;
use config::Config;

const USAGE: &str = "\
rnano — a minimal, nano-like terminal text editor

USAGE:
    rnano [OPTIONS] [FILE [FILE ...]]

OPTIONS:
    -v, --readonly    Open all files read-only (no edits, no saving)
    -B, --backup      Save a backup to <FILE>~ before overwriting
    -V, --version     Print version and exit
    -h, --help        Print this help and exit

KEYS:
    Ctrl+O Write    Ctrl+X Exit     Ctrl+G Help
    Ctrl+K Cut      Ctrl+U Paste    Ctrl+A/E Start/end of line
    Alt+N  Next buf Alt+B  Prev buf
    Ctrl+R Rec macro  Alt+R Play macro

CONFIG:
    Linux/macOS: $XDG_CONFIG_HOME/rnano/config.toml
                 (defaults to ~/.config/rnano/config.toml)
    Windows:     %APPDATA%\\rnano\\config.toml
    CLI flags override values set in the config file.
";

fn main() -> ExitCode {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut readonly = false;
    let mut backup_flag = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("rnano {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-v" | "--readonly" | "--view" => readonly = true,
            "-B" | "--backup" => backup_flag = true,
            s if s.starts_with('-') && s.len() > 1 => {
                eprintln!("rnano: unknown option '{s}' (try --help)");
                return ExitCode::from(2);
            }
            s => paths.push(PathBuf::from(s)),
        }
    }

    // Load config file; warn on parse error but keep going with defaults.
    let mut cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rnano: config warning: {e}");
            Config::default()
        }
    };
    cfg.apply_cli(backup_flag);

    let bufs = if paths.is_empty() {
        vec![Buffer::new(readonly)]
    } else {
        let mut v = Vec::with_capacity(paths.len());
        for p in &paths {
            match Buffer::from_path(p, readonly) {
                Ok(b) => v.push(b),
                Err(e) => {
                    eprintln!("rnano: {}: {}", p.display(), e);
                    return ExitCode::FAILURE;
                }
            }
        }
        v
    };

    editor::install_panic_hook();
    if let Err(e) = editor::run(bufs, cfg) {
        eprintln!("rnano: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
