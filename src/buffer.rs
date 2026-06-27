//! Text buffer backed by a [`ropey::Rope`].
//!
//! The buffer normalizes line endings to `\n` in memory (CRLF is detected on
//! load and restored on save) so the cursor and line model stay predictable.
//! Saving is atomic: content is written to a temp file in the same directory,
//! fsynced, and renamed over the target, preserving the original permissions.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ropey::Rope;

/// Line-ending style of a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
        }
    }

    fn platform_default() -> Eol {
        if cfg!(windows) { Eol::Crlf } else { Eol::Lf }
    }
}

/// An in-memory text buffer plus the metadata needed to save it faithfully.
#[derive(Debug)]
pub struct Buffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub eol: Eol,
    pub dirty: bool,
    pub readonly: bool,
    /// True when `path` was given but the file did not exist yet.
    pub is_new: bool,
}

impl Buffer {
    /// An empty, unnamed buffer.
    pub fn new(readonly: bool) -> Buffer {
        Buffer {
            rope: Rope::new(),
            path: None,
            eol: Eol::platform_default(),
            dirty: false,
            readonly,
            is_new: false,
        }
    }

    /// Load a file. A missing file yields an empty buffer pointed at `path`
    /// (a "new file"). Binary files and non-UTF-8 content are rejected.
    pub fn from_path(path: &Path, readonly: bool) -> io::Result<Buffer> {
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "is a directory",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Buffer {
                    rope: Rope::new(),
                    path: Some(path.to_path_buf()),
                    eol: Eol::platform_default(),
                    dirty: false,
                    readonly,
                    is_new: true,
                });
            }
            Err(e) => return Err(e),
        }

        let bytes = fs::read(path).map_err(|e| annotate_read_error(path, e))?;
        if bytes.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "appears to be a binary file (contains NUL bytes)",
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "not valid UTF-8 — use --encoding (planned) to open legacy files",
            )
        })?;

        let eol = if text.contains("\r\n") {
            Eol::Crlf
        } else {
            Eol::Lf
        };
        let normalized = if eol == Eol::Crlf {
            text.replace("\r\n", "\n")
        } else {
            text
        };

        Ok(Buffer {
            rope: Rope::from_str(&normalized),
            path: Some(path.to_path_buf()),
            eol,
            dirty: false,
            readonly,
            is_new: false,
        })
    }

    // --- line / index helpers -------------------------------------------------

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line(&self, row: usize) -> ropey::RopeSlice<'_> {
        self.rope.line(row)
    }

    /// Number of chars in `row`, excluding a trailing `\n`.
    pub fn line_len_chars(&self, row: usize) -> usize {
        let line = self.rope.line(row);
        let mut n = line.len_chars();
        if n > 0 && line.char(n - 1) == '\n' {
            n -= 1;
        }
        n
    }

    /// Absolute char index of `(row, col)`.
    pub fn char_idx(&self, row: usize, col: usize) -> usize {
        self.rope.line_to_char(row) + col
    }

    // --- edits ----------------------------------------------------------------

    pub fn insert_char(&mut self, char_idx: usize, ch: char) {
        self.rope.insert_char(char_idx, ch);
        self.dirty = true;
    }

    pub fn insert(&mut self, char_idx: usize, text: &str) {
        self.rope.insert(char_idx, text);
        self.dirty = true;
    }

    pub fn remove(&mut self, range: std::ops::Range<usize>) {
        self.rope.remove(range);
        self.dirty = true;
    }

    // --- saving ---------------------------------------------------------------

    /// Atomically write the buffer to `path`. Restores the buffer's EOL style,
    /// preserves the target's permissions when it already exists, and (with
    /// `backup`) copies the original to `<path>~` first.
    pub fn save(&mut self, path: &Path, backup: bool) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let mut data = self.rope.to_string();
        if self.eol == Eol::Crlf {
            data = data.replace('\n', "\r\n");
        }

        let target_exists = path.try_exists().unwrap_or(false);

        if backup && target_exists {
            let mut bak = path.as_os_str().to_os_string();
            bak.push("~");
            fs::copy(path, PathBuf::from(bak))?;
        }

        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "rnano".to_string());
        let tmp = parent.join(format!(".{}.rnano-{}.tmp", file_name, std::process::id()));

        let write_tmp = || -> io::Result<()> {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(data.as_bytes())?;
            f.flush()?;
            f.sync_all()?;
            if target_exists {
                if let Ok(meta) = fs::metadata(path) {
                    let _ = fs::set_permissions(&tmp, meta.permissions());
                }
            }
            Ok(())
        };

        if let Err(e) = write_tmp() {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }

        if let Err(e) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }

        // Best-effort durability of the rename itself.
        #[cfg(unix)]
        {
            if let Ok(dir) = fs::File::open(&parent) {
                let _ = dir.sync_all();
            }
        }

        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.is_new = false;
        Ok(())
    }

    /// Write the buffer to `dest` without changing `self.dirty` or `self.path`.
    /// Used by autosave to write a swap file.
    pub fn save_copy(&self, dest: &Path) -> io::Result<()> {
        use std::io::Write as IoWrite;
        let mut data = self.rope.to_string();
        if self.eol == Eol::Crlf {
            data = data.replace('\n', "\r\n");
        }
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dest)?;
        f.write_all(data.as_bytes())?;
        f.flush()?;
        f.sync_all()
    }
}

/// Annotate a read error with an actionable hint for common cases.
fn annotate_read_error(path: &Path, e: io::Error) -> io::Error {
    match e.kind() {
        io::ErrorKind::PermissionDenied => io::Error::new(
            e.kind(),
            format!(
                "{}: permission denied — try opening with --readonly or check file permissions",
                path.display()
            ),
        ),
        _ => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("rnano-test-{}-{}", std::process::id(), name));
        fs::create_dir_all(&dir).unwrap();
        dir.join("file.txt")
    }

    #[test]
    fn edits_and_indices() {
        let mut b = Buffer::new(false);
        b.insert(0, "abc\ndef");
        assert_eq!(b.len_lines(), 2);
        assert_eq!(b.line_len_chars(0), 3);
        assert_eq!(b.line_len_chars(1), 3);
        assert_eq!(b.char_idx(1, 1), 5);
        b.remove(0..1); // delete 'a'
        assert_eq!(b.rope.to_string(), "bc\ndef");
        assert!(b.dirty);
    }

    #[test]
    fn binary_files_are_rejected() {
        let path = scratch("binary");
        fs::write(&path, b"ok\x00then").unwrap();
        let err = Buffer::from_path(&path, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn crlf_is_detected_and_restored() {
        let path = scratch("crlf");
        fs::write(&path, b"one\r\ntwo\r\n").unwrap();
        let mut b = Buffer::from_path(&path, false).unwrap();
        assert_eq!(b.eol, Eol::Crlf);
        // In memory it is normalized to LF.
        assert_eq!(b.rope.to_string(), "one\ntwo\n");
        b.save(&path, false).unwrap();
        let raw = fs::read(&path).unwrap();
        assert_eq!(raw, b"one\r\ntwo\r\n");
    }

    #[test]
    fn atomic_save_round_trips_and_clears_dirty() {
        let path = scratch("roundtrip");
        let mut b = Buffer::new(false);
        b.insert(0, "hello\nworld");
        assert!(b.dirty);
        b.save(&path, false).unwrap();
        assert!(!b.dirty);
        let reloaded = Buffer::from_path(&path, false).unwrap();
        assert_eq!(reloaded.rope.to_string(), "hello\nworld");
    }

    #[test]
    fn missing_file_is_a_new_buffer() {
        let path = scratch("missing");
        let target = path.with_file_name("does-not-exist.txt");
        let b = Buffer::from_path(&target, false).unwrap();
        assert!(b.is_new);
        assert_eq!(b.rope.len_chars(), 0);
        assert_eq!(b.path.as_deref(), Some(target.as_path()));
    }

    #[test]
    fn non_utf8_is_rejected() {
        let path = scratch("latin1");
        fs::write(&path, b"\xff\xfe hello").unwrap();
        let err = Buffer::from_path(&path, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn directory_is_rejected() {
        let path = scratch("dir");
        fs::create_dir_all(&path).unwrap();
        let err = Buffer::from_path(&path, false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    // Property-style tests: sequences of insert/delete must keep invariants.

    #[test]
    fn insert_sequence_keeps_char_count() {
        let mut b = Buffer::new(false);
        let ops = ["hello", "\n", "world", "\n", "!"];
        let mut expected_chars = 0usize;
        for s in ops {
            b.insert(b.rope.len_chars(), s);
            expected_chars += s.chars().count();
            assert_eq!(b.rope.len_chars(), expected_chars);
        }
    }

    #[test]
    fn delete_sequence_reduces_char_count() {
        let mut b = Buffer::new(false);
        b.insert(0, "abcdef");
        assert_eq!(b.rope.len_chars(), 6);
        b.remove(2..4); // remove "cd"
        assert_eq!(b.rope.len_chars(), 4);
        assert_eq!(b.rope.to_string(), "abef");
        b.remove(0..1); // remove "a"
        assert_eq!(b.rope.to_string(), "bef");
    }

    #[test]
    fn insert_at_various_positions_keeps_content() {
        // Invariant: content is exactly the concatenation of inserts in order.
        let mut b = Buffer::new(false);
        b.insert(0, "ac");
        b.insert(1, "b"); // insert between 'a' and 'c'
        assert_eq!(b.rope.to_string(), "abc");
        b.insert(3, "d");
        assert_eq!(b.rope.to_string(), "abcd");
        b.insert(0, "Z");
        assert_eq!(b.rope.to_string(), "Zabcd");
    }

    #[test]
    fn line_len_excludes_newline() {
        let mut b = Buffer::new(false);
        b.insert(0, "abc\nde\n");
        assert_eq!(b.line_len_chars(0), 3);
        assert_eq!(b.line_len_chars(1), 2);
        // Last line (empty, after trailing \n)
        assert_eq!(b.line_len_chars(2), 0);
    }

    #[test]
    fn char_idx_matches_rope_offset() {
        let mut b = Buffer::new(false);
        b.insert(0, "ab\ncd");
        // (row=0, col=0) -> char 0
        assert_eq!(b.char_idx(0, 0), 0);
        // (row=0, col=2) -> char 2 (the 'b')  wait, 'a'=0, 'b'=1, '\n'=2
        assert_eq!(b.char_idx(0, 2), 2);
        // (row=1, col=0) -> char 3 (the 'c')
        assert_eq!(b.char_idx(1, 0), 3);
        // (row=1, col=1) -> char 4 (the 'd')
        assert_eq!(b.char_idx(1, 1), 4);
    }

    #[test]
    fn save_with_backup_creates_tilde_file() {
        let path = scratch("backup");
        let mut b = Buffer::new(false);
        b.insert(0, "original");
        b.save(&path, false).unwrap();
        // Write again with backup.
        b.insert(b.rope.len_chars(), " updated");
        b.save(&path, true).unwrap();
        let bak = path.with_file_name(format!("{}~", path.file_name().unwrap().to_string_lossy()));
        assert!(bak.exists(), "backup file should exist");
        let bak_content = fs::read_to_string(&bak).unwrap();
        assert_eq!(bak_content, "original");
    }

    #[test]
    fn save_lf_only_does_not_add_crlf() {
        let path = scratch("lf_only");
        let mut b = Buffer::new(false);
        b.insert(0, "line1\nline2\n");
        b.save(&path, false).unwrap();
        let raw = fs::read(&path).unwrap();
        assert!(!raw.contains(&b'\r'), "LF-only save must not contain CR");
    }

    #[test]
    fn dirty_cleared_only_after_save() {
        let mut b = Buffer::new(false);
        b.insert(0, "x");
        assert!(b.dirty);
        let path = scratch("dirty");
        b.save(&path, false).unwrap();
        assert!(!b.dirty);
    }
}
