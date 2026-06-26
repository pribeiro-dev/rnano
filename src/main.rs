mod buffer;
mod editor;

use std::path::PathBuf;
use std::process::ExitCode;

use buffer::Buffer;

const USAGE: &str = "\
rnano — a minimal, nano-like terminal text editor

USAGE:
    rnano [OPTIONS] [FILE]

OPTIONS:
    -v, --readonly    Open the file read-only (no edits, no saving)
    -B, --backup      Save a backup to <FILE>~ before overwriting
    -V, --version     Print version and exit
    -h, --help        Print this help and exit

KEYS:
    Ctrl+O Write    Ctrl+X Exit    Ctrl+G Help
    Ctrl+K Cut line Ctrl+U Paste   Ctrl+A/E Start/end of line
";

fn main() -> ExitCode {
    let mut path: Option<PathBuf> = None;
    let mut readonly = false;
    let mut backup = false;

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
            "-B" | "--backup" => backup = true,
            s if s.starts_with('-') && s.len() > 1 => {
                eprintln!("rnano: unknown option '{s}' (try --help)");
                return ExitCode::from(2);
            }
            s => {
                if path.is_none() {
                    path = Some(PathBuf::from(s));
                } else {
                    eprintln!("rnano: only one file is supported for now; ignoring '{s}'");
                }
            }
        }
    }

    let buf = match &path {
        Some(p) => match Buffer::from_path(p, readonly) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rnano: {}: {}", p.display(), e);
                return ExitCode::FAILURE;
            }
        },
        None => Buffer::new(readonly),
    };

    editor::install_panic_hook();
    if let Err(e) = editor::run(buf, backup) {
        eprintln!("rnano: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
