//! Terminal editor: viewport, cursor, input handling, and the render loop.
//!
//! Screen layout (top to bottom):
//! ```text
//!   row 0           title bar (reverse video)
//!   rows 1..H-2     text area
//!   row H-2         status / prompt line
//!   row H-1         key-hint line
//! ```

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};
use ropey::RopeSlice;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::buffer::Buffer;
use crate::config::Config;
use crate::highlight;
use crate::history::{History, HistoryEntry, Op};

// --- keymap table (issue #4) --------------------------------------------------

const HELP_LINES: &[&str] = &[
    " rnano — keyboard shortcuts",
    "",
    "  Navigation",
    "    Arrows / Home / End / PgUp / PgDn   Move cursor",
    "    Ctrl+A / Ctrl+E                     Start / end of line",
    "",
    "  Editing",
    "    Ctrl+K                              Cut line",
    "    Ctrl+U                              Uncut (paste from kill ring)",
    "    Alt+W                               Copy kill ring to OS clipboard",
    "    Alt+U                               Paste from OS clipboard",
    "    Ctrl+Z                              Undo",
    "    Ctrl+Y                              Redo",
    "",
    "  Macros",
    "    Ctrl+R                              Start / stop macro recording",
    "    Alt+R                               Play back last macro",
    "",
    "  File",
    "    Ctrl+O                              Write / Save as",
    "    Ctrl+S                              Save (current filename)",
    "    Ctrl+X                              Exit (prompts if modified)",
    "",
    "  Buffers",
    "    Alt+N                               Next buffer",
    "    Alt+B                               Previous buffer",
    "",
    "  Search",
    "    Ctrl+W                              Incremental search",
    "    Ctrl+W  (in search)                 Next match",
    "    Ctrl+Q  (in search)                 Previous match",
    "    Alt+C   (in search)                 Toggle case sensitivity",
    "",
    "  View",
    "    Ctrl+N                              Toggle line numbers",
    "    Ctrl+P                              Toggle soft wrap",
    "",
    "  Misc",
    "    Ctrl+C                              Show cursor position",
    "    Ctrl+G                              This help (↑↓ to scroll, any key closes)",
    "",
    "  Press any key to return.",
];

const HINT_LINE: &str =
    " ^O Write  ^X Exit  ^G Help  ^K Cut  ^U Paste  ^W Search  ^Z Undo  ^Y Redo";

// --- themes -------------------------------------------------------------------

/// A palette of colours for the editor UI and syntax tokens.
#[derive(Clone)]
pub struct Theme {
    pub title_bg: Color,
    pub status_bg: Color,
    pub gutter_fg: Color,
    pub search_hl_bg: Color,
    pub search_hl_fg: Color,
}

impl Theme {
    fn dark() -> Self {
        Theme {
            title_bg: Color::Blue,
            status_bg: Color::Blue,
            gutter_fg: Color::DarkGrey,
            search_hl_bg: Color::Yellow,
            search_hl_fg: Color::Black,
        }
    }

    fn light() -> Self {
        Theme {
            title_bg: Color::White,
            status_bg: Color::White,
            gutter_fg: Color::Grey,
            search_hl_bg: Color::Blue,
            search_hl_fg: Color::White,
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "light" => Theme::light(),
            _ => Theme::dark(),
        }
    }
}

// --- focus / prompt -----------------------------------------------------------

enum Focus {
    Edit,
    Prompt(Prompt),
}

struct Prompt {
    label: String,
    input: String,
    kind: PromptKind,
}

enum PromptKind {
    SaveAs {
        quit_after: bool,
    },
    ConfirmQuit,
    Search {
        case_insensitive: bool,
        matches: Vec<(usize, usize, usize)>,
        current: Option<usize>,
        saved_row: usize,
        saved_col: usize,
    },
}

// --- editor -------------------------------------------------------------------

pub struct Editor {
    buf: Buffer,
    cursor_row: usize,
    cursor_col: usize,
    goal_col: usize,
    top: usize,
    left: usize,
    width: usize,
    height: usize,
    message: String,
    focus: Focus,
    help_visible: bool,
    help_scroll: usize,
    should_quit: bool,
    kill: Option<String>,
    last_search: String,
    history: History,
    cfg: Config,
    theme: Theme,
    /// Extension-based syntax rules (empty = no highlighting).
    syntax_rules: &'static [highlight::Rule],
    /// Whether highlighting is suppressed due to file size.
    highlight_disabled: bool,
    /// When autosave is enabled, the last time a periodic save was attempted.
    last_autosave: Option<Instant>,
    // --- multiple buffers (#32) -----------------------------------------------
    /// Parked buffers: (buf, cursor_row, cursor_col, goal_col, top, left).
    parked_bufs: VecDeque<(Buffer, usize, usize, usize, usize, usize)>,
    /// Index of the current buffer in the logical ordered list.
    buf_idx: usize,
    /// Total number of open buffers (1 + parked_bufs.len()).
    buf_total: usize,
    // --- macros (#33) ---------------------------------------------------------
    macro_recording: bool,
    macro_buf: Vec<KeyEvent>,
    macro_saved: Vec<KeyEvent>,
}

impl Editor {
    pub fn new(bufs: Vec<Buffer>, width: usize, height: usize, cfg: Config) -> Editor {
        let mut it = bufs.into_iter();
        let buf = it.next().unwrap_or_else(|| Buffer::new(false));
        let parked_bufs: VecDeque<_> = it.map(|b| (b, 0, 0, 0, 0, 0)).collect();
        let buf_total = parked_bufs.len() + 1;

        let message = if buf.is_new {
            "New file".to_string()
        } else if buf.readonly {
            "Read-only mode".to_string()
        } else {
            String::new()
        };
        let theme = Theme::from_name(&cfg.theme);
        let (syntax_rules, highlight_disabled) = syntax_for_buf(&buf, &cfg);
        let last_autosave = if cfg.autosave_interval > 0 {
            Some(Instant::now())
        } else {
            None
        };
        Editor {
            buf,
            cursor_row: 0,
            cursor_col: 0,
            goal_col: 0,
            top: 0,
            left: 0,
            width: width.max(1),
            height: height.max(4),
            message,
            focus: Focus::Edit,
            help_visible: false,
            help_scroll: 0,
            should_quit: false,
            kill: None,
            last_search: String::new(),
            history: History::new(cfg.undo_depth),
            theme,
            syntax_rules,
            highlight_disabled,
            last_autosave,
            parked_bufs,
            buf_idx: 0,
            buf_total,
            macro_recording: false,
            macro_buf: Vec::new(),
            macro_saved: Vec::new(),
            cfg,
        }
    }

    fn text_height(&self) -> usize {
        self.height.saturating_sub(3).max(1)
    }

    /// Width of the line-number gutter (0 when disabled).
    fn gutter_width(&self) -> usize {
        if !self.cfg.line_numbers {
            return 0;
        }
        format!("{}", self.buf.len_lines()).len() + 1
    }

    /// Usable text columns (viewport width minus gutter).
    fn text_width(&self) -> usize {
        self.width.saturating_sub(self.gutter_width()).max(1)
    }

    // --- cursor / movement ----------------------------------------------------

    fn cur_idx(&self) -> usize {
        self.buf.char_idx(self.cursor_row, self.cursor_col)
    }

    fn cursor_display_col(&self) -> usize {
        display_col(
            self.buf.line(self.cursor_row),
            self.cursor_col,
            self.cfg.tab_width,
        )
    }

    fn update_goal(&mut self) {
        self.goal_col = self.cursor_display_col();
    }

    fn snap_to_goal(&mut self) {
        let max = self.buf.line_len_chars(self.cursor_row);
        self.cursor_col = char_at_display(
            self.buf.line(self.cursor_row),
            self.goal_col,
            max,
            self.cfg.tab_width,
        );
    }

    fn clamp_cursor(&mut self) {
        let lines = self.buf.len_lines();
        if self.cursor_row >= lines {
            self.cursor_row = lines.saturating_sub(1);
        }
        self.cursor_col = self
            .cursor_col
            .min(self.buf.line_len_chars(self.cursor_row));
        self.update_goal();
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.buf.line_len_chars(self.cursor_row);
        }
        self.update_goal();
    }

    fn move_right(&mut self) {
        let len = self.buf.line_len_chars(self.cursor_row);
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.buf.len_lines() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        self.update_goal();
    }

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.snap_to_goal();
        }
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.buf.len_lines() {
            self.cursor_row += 1;
            self.snap_to_goal();
        }
    }

    fn move_home(&mut self) {
        self.cursor_col = 0;
        self.update_goal();
    }

    fn move_end(&mut self) {
        self.cursor_col = self.buf.line_len_chars(self.cursor_row);
        self.update_goal();
    }

    fn page_up(&mut self) {
        let h = self.text_height();
        self.cursor_row = self.cursor_row.saturating_sub(h);
        self.snap_to_goal();
    }

    fn page_down(&mut self) {
        let h = self.text_height();
        self.cursor_row = (self.cursor_row + h).min(self.buf.len_lines().saturating_sub(1));
        self.snap_to_goal();
    }

    // --- editing --------------------------------------------------------------

    fn ensure_writable(&mut self) -> bool {
        if self.buf.readonly {
            self.message = "Buffer is read-only (opened with --readonly)".to_string();
            false
        } else {
            true
        }
    }

    fn insert_char(&mut self, ch: char) {
        if !self.ensure_writable() {
            return;
        }
        let before = (self.cursor_row, self.cursor_col);
        let idx = self.cur_idx();
        self.buf.insert_char(idx, ch);
        self.cursor_col += 1;
        self.update_goal();
        let after = (self.cursor_row, self.cursor_col);
        if !self.history.try_squash_insert(idx, ch, after) {
            self.history.push(HistoryEntry {
                to_undo: Op::Delete {
                    pos: idx,
                    char_len: 1,
                },
                to_redo: Op::Insert {
                    pos: idx,
                    text: ch.to_string(),
                },
                before,
                after,
            });
        }
    }

    fn insert_newline(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let before = (self.cursor_row, self.cursor_col);
        let idx = self.cur_idx();
        self.buf.insert_char(idx, '\n');
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.update_goal();
        let after = (self.cursor_row, self.cursor_col);
        self.history.push(HistoryEntry {
            to_undo: Op::Delete {
                pos: idx,
                char_len: 1,
            },
            to_redo: Op::Insert {
                pos: idx,
                text: "\n".to_string(),
            },
            before,
            after,
        });
    }

    fn delete_back(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let before = (self.cursor_row, self.cursor_col);
        if self.cursor_col > 0 {
            let idx = self.cur_idx();
            let deleted = self.buf.rope.slice(idx - 1..idx).to_string();
            self.buf.remove(idx - 1..idx);
            self.cursor_col -= 1;
            self.update_goal();
            self.history.push(HistoryEntry {
                to_undo: Op::Insert {
                    pos: idx - 1,
                    text: deleted.clone(),
                },
                to_redo: Op::Delete {
                    pos: idx - 1,
                    char_len: 1,
                },
                before,
                after: (self.cursor_row, self.cursor_col),
            });
        } else if self.cursor_row > 0 {
            let prev_len = self.buf.line_len_chars(self.cursor_row - 1);
            let idx = self.cur_idx();
            let deleted = self.buf.rope.slice(idx - 1..idx).to_string();
            self.buf.remove(idx - 1..idx);
            self.cursor_row -= 1;
            self.cursor_col = prev_len;
            self.update_goal();
            self.history.push(HistoryEntry {
                to_undo: Op::Insert {
                    pos: idx - 1,
                    text: deleted,
                },
                to_redo: Op::Delete {
                    pos: idx - 1,
                    char_len: 1,
                },
                before,
                after: (self.cursor_row, self.cursor_col),
            });
        }
    }

    fn delete_forward(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let before = (self.cursor_row, self.cursor_col);
        let line_len = self.buf.line_len_chars(self.cursor_row);
        let idx = self.cur_idx();
        if self.cursor_col < line_len || self.cursor_row + 1 < self.buf.len_lines() {
            let deleted = self.buf.rope.slice(idx..idx + 1).to_string();
            self.buf.remove(idx..idx + 1);
            self.update_goal();
            self.history.push(HistoryEntry {
                to_undo: Op::Insert {
                    pos: idx,
                    text: deleted,
                },
                to_redo: Op::Delete {
                    pos: idx,
                    char_len: 1,
                },
                before,
                after: (self.cursor_row, self.cursor_col),
            });
        }
    }

    fn cut_line(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let before = (self.cursor_row, self.cursor_col);
        let row = self.cursor_row;
        let start = self.buf.rope.line_to_char(row);
        let end = if row + 1 < self.buf.len_lines() {
            self.buf.rope.line_to_char(row + 1)
        } else {
            self.buf.rope.len_chars()
        };
        let cut_text = self.buf.rope.slice(start..end).to_string();
        self.kill = Some(cut_text.clone());
        self.buf.remove(start..end);
        self.clamp_cursor();
        self.history.push(HistoryEntry {
            to_undo: Op::Insert {
                pos: start,
                text: cut_text,
            },
            to_redo: Op::Delete {
                pos: start,
                char_len: end - start,
            },
            before,
            after: (self.cursor_row, self.cursor_col),
        });
        self.message = "Cut line".to_string();
    }

    fn paste(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let Some(text) = self.kill.clone() else {
            self.message = "Nothing to paste".to_string();
            return;
        };
        let before = (self.cursor_row, self.cursor_col);
        let idx = self.cur_idx();
        let char_count = text.chars().count();
        self.buf.insert(idx, &text);
        let new_idx = idx + char_count;
        self.cursor_row = self.buf.rope.char_to_line(new_idx);
        self.cursor_col = new_idx - self.buf.rope.line_to_char(self.cursor_row);
        self.update_goal();
        self.history.push(HistoryEntry {
            to_undo: Op::Delete {
                pos: idx,
                char_len: char_count,
            },
            to_redo: Op::Insert {
                pos: idx,
                text: text.clone(),
            },
            before,
            after: (self.cursor_row, self.cursor_col),
        });
        self.message = "Uncut text".to_string();
    }

    // --- undo / redo (#16) ----------------------------------------------------

    fn undo(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let Some(entry) = self.history.pop_undo() else {
            self.message = "Nothing to undo".to_string();
            return;
        };
        apply_op(&mut self.buf, &entry.to_undo);
        self.cursor_row = entry.before.0;
        self.cursor_col = entry.before.1;
        self.clamp_cursor();
        self.message = "Undone".to_string();
    }

    fn redo(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let Some(entry) = self.history.pop_redo() else {
            self.message = "Nothing to redo".to_string();
            return;
        };
        apply_op(&mut self.buf, &entry.to_redo);
        self.cursor_row = entry.after.0;
        self.cursor_col = entry.after.1;
        self.clamp_cursor();
        self.message = "Redone".to_string();
    }

    // --- saving / quitting ----------------------------------------------------

    fn request_quit(&mut self) {
        if self.buf.dirty {
            self.focus = Focus::Prompt(Prompt {
                label: "Save modified buffer? (Y/N)".to_string(),
                input: String::new(),
                kind: PromptKind::ConfirmQuit,
            });
        } else {
            self.should_quit = true;
        }
    }

    fn quick_save(&mut self, quit_after: bool) {
        if self.buf.readonly {
            self.message = "Cannot save: read-only mode".to_string();
            return;
        }
        match self.buf.path.clone() {
            Some(p) => self.do_save(&p, quit_after),
            None => self.prompt_save(quit_after),
        }
    }

    fn prompt_save(&mut self, quit_after: bool) {
        if self.buf.readonly {
            self.message = "Cannot save: read-only mode".to_string();
            return;
        }
        let input = self
            .buf
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.focus = Focus::Prompt(Prompt {
            label: "File name to write".to_string(),
            input,
            kind: PromptKind::SaveAs { quit_after },
        });
    }

    fn do_save(&mut self, path: &Path, quit_after: bool) {
        match self.buf.save(path, self.cfg.backup) {
            Ok(()) => {
                self.message = format!("Wrote \"{}\"", path.display());
                self.run_save_hook(path);
                if quit_after {
                    self.should_quit = true;
                }
            }
            Err(e) => {
                self.message = actionable_save_error(path, &e);
            }
        }
    }

    /// Run `cfg.on_save_hook` after a successful save (#34).
    fn run_save_hook(&mut self, path: &Path) {
        if self.cfg.on_save_hook.is_empty() {
            return;
        }
        let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let cmd = self
            .cfg
            .on_save_hook
            .replace("%f", &abs.display().to_string());
        match std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => {
                self.message = format!("Hook exited {}", s.code().unwrap_or(-1));
            }
            Err(e) => {
                self.message = format!("Hook failed: {e}");
            }
        }
    }

    /// Autosave to a swap file (`.filename.swp`) if the buffer is dirty
    /// and the interval has elapsed (#22).
    fn maybe_autosave(&mut self) {
        let Some(last) = self.last_autosave else {
            return;
        };
        let interval = Duration::from_secs(self.cfg.autosave_interval);
        if last.elapsed() < interval {
            return;
        }
        self.last_autosave = Some(Instant::now());
        if !self.buf.dirty || self.buf.readonly {
            return;
        }
        let Some(ref orig_path) = self.buf.path.clone() else {
            return;
        };
        let parent = orig_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let name = orig_path
            .file_name()
            .map(|n| format!(".{}.swp", n.to_string_lossy()))
            .unwrap_or_else(|| ".rnano.swp".to_string());
        let swap = parent.join(name);
        if let Err(e) = self.buf.save_copy(&swap) {
            self.message = format!("Autosave failed: {e}");
        }
    }

    // --- search ---------------------------------------------------------------

    fn open_search(&mut self) {
        let saved_row = self.cursor_row;
        let saved_col = self.cursor_col;
        let initial = self.last_search.clone();
        let matches = search_matches(&self.buf.rope, &initial, false);
        let current = first_match_at_or_after(&matches, saved_row, saved_col);
        if let Some(idx) = current {
            let (r, c, _) = matches[idx];
            self.cursor_row = r;
            self.cursor_col = c;
            self.update_goal();
        }
        self.focus = Focus::Prompt(Prompt {
            label: "Search".to_string(),
            input: initial,
            kind: PromptKind::Search {
                case_insensitive: false,
                matches,
                current,
                saved_row,
                saved_col,
            },
        });
    }

    fn search_step(&mut self, forward: bool) {
        let Focus::Prompt(ref mut p) = self.focus else {
            return;
        };
        let PromptKind::Search {
            ref matches,
            ref mut current,
            ..
        } = p.kind
        else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        let next = match *current {
            None => 0,
            Some(i) => {
                if forward {
                    (i + 1) % matches.len()
                } else {
                    i.checked_sub(1).unwrap_or(matches.len() - 1)
                }
            }
        };
        *current = Some(next);
        let (r, c, _) = matches[next];
        self.cursor_row = r;
        self.cursor_col = c;
        self.update_goal();
    }

    fn search_close(&mut self, accept: bool) {
        let Focus::Prompt(ref p) = self.focus else {
            return;
        };
        let PromptKind::Search {
            saved_row,
            saved_col,
            ref matches,
            current,
            ..
        } = p.kind
        else {
            return;
        };
        let query = p.input.clone();
        let (saved_row, saved_col) = (saved_row, saved_col);
        if accept {
            if let Some(idx) = current {
                let (r, c, _) = matches[idx];
                self.cursor_row = r;
                self.cursor_col = c;
            }
            self.last_search = query;
            self.message = if matches.is_empty() {
                "No matches".to_string()
            } else {
                String::new()
            };
        } else {
            self.cursor_row = saved_row;
            self.cursor_col = saved_col;
            self.message = "Search cancelled".to_string();
        }
        self.update_goal();
        self.focus = Focus::Edit;
    }

    fn search_edit(&mut self, f: impl FnOnce(&mut String, &mut bool) -> String) {
        let mut prompt = match std::mem::replace(&mut self.focus, Focus::Edit) {
            Focus::Prompt(p) => p,
            Focus::Edit => return,
        };
        let (new_matches, new_current) = {
            let PromptKind::Search {
                ref mut case_insensitive,
                saved_row,
                saved_col,
                ..
            } = prompt.kind
            else {
                self.focus = Focus::Prompt(prompt);
                return;
            };
            let new_query = f(&mut prompt.input, case_insensitive);
            let ci = *case_insensitive;
            let m = search_matches(&self.buf.rope, &new_query, ci);
            let cur = first_match_at_or_after(&m, saved_row, saved_col);
            (m, cur)
        };
        if let Some(idx) = new_current {
            let (r, c, _) = new_matches[idx];
            self.cursor_row = r;
            self.cursor_col = c;
            self.update_goal();
        }
        let PromptKind::Search {
            ref mut matches,
            ref mut current,
            ..
        } = prompt.kind
        else {
            unreachable!()
        };
        *matches = new_matches;
        *current = new_current;
        self.focus = Focus::Prompt(prompt);
    }

    // --- multiple buffers (#32) -----------------------------------------------

    fn next_buffer(&mut self) {
        if self.buf_total <= 1 {
            self.message = "Only one buffer open".to_string();
            return;
        }
        // Take next-in-line from the front of the queue.
        let (new_buf, nr, nc, ng, nt, nl) = self.parked_bufs.pop_front().unwrap();
        // Park current at the back.
        let old_buf = std::mem::replace(&mut self.buf, new_buf);
        self.parked_bufs.push_back((
            old_buf,
            self.cursor_row,
            self.cursor_col,
            self.goal_col,
            self.top,
            self.left,
        ));
        self.cursor_row = nr;
        self.cursor_col = nc;
        self.goal_col = ng;
        self.top = nt;
        self.left = nl;
        self.buf_idx = (self.buf_idx + 1) % self.buf_total;
        let (rules, dis) = syntax_for_buf(&self.buf, &self.cfg);
        self.syntax_rules = rules;
        self.highlight_disabled = dis;
        self.history = History::new(self.cfg.undo_depth);
        self.message = format!("Buffer {}/{}", self.buf_idx + 1, self.buf_total);
    }

    fn prev_buffer(&mut self) {
        if self.buf_total <= 1 {
            self.message = "Only one buffer open".to_string();
            return;
        }
        // Take prev-in-line from the back of the queue.
        let (new_buf, nr, nc, ng, nt, nl) = self.parked_bufs.pop_back().unwrap();
        let old_buf = std::mem::replace(&mut self.buf, new_buf);
        self.parked_bufs.push_front((
            old_buf,
            self.cursor_row,
            self.cursor_col,
            self.goal_col,
            self.top,
            self.left,
        ));
        self.cursor_row = nr;
        self.cursor_col = nc;
        self.goal_col = ng;
        self.top = nt;
        self.left = nl;
        self.buf_idx = self.buf_idx.checked_sub(1).unwrap_or(self.buf_total - 1);
        let (rules, dis) = syntax_for_buf(&self.buf, &self.cfg);
        self.syntax_rules = rules;
        self.highlight_disabled = dis;
        self.history = History::new(self.cfg.undo_depth);
        self.message = format!("Buffer {}/{}", self.buf_idx + 1, self.buf_total);
    }

    // --- macros (#33) ---------------------------------------------------------

    fn macro_toggle_record(&mut self) {
        if self.macro_recording {
            self.macro_recording = false;
            // Drop the last key (the Ctrl+R that stopped recording).
            self.macro_buf.pop();
            self.macro_saved = std::mem::take(&mut self.macro_buf);
            self.message = format!("Macro recorded ({} keys)", self.macro_saved.len());
        } else {
            self.macro_recording = true;
            self.macro_buf.clear();
            self.message = "Recording macro…".to_string();
        }
    }

    fn macro_play(&mut self) {
        if self.macro_saved.is_empty() {
            self.message = "No macro recorded (use Ctrl+R to record)".to_string();
            return;
        }
        if self.macro_recording {
            self.message = "Cannot play macro while recording".to_string();
            return;
        }
        let keys = self.macro_saved.clone();
        for key in keys {
            self.handle_key(key);
            if self.should_quit {
                break;
            }
        }
    }

    // --- mouse (#31) ----------------------------------------------------------

    fn handle_mouse(&mut self, e: MouseEvent) {
        match e.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let sr = e.row as usize;
                let sc = e.column as usize;
                let gutter = self.gutter_width();
                // Ignore clicks on title/status/hint bars.
                if sr == 0 || sr + 2 >= self.height {
                    return;
                }
                let text_row = sr - 1 + self.top;
                if text_row >= self.buf.len_lines() {
                    return;
                }
                self.cursor_row = text_row;
                let target_col = (sc + self.left).saturating_sub(gutter);
                let max = self.buf.line_len_chars(self.cursor_row);
                self.cursor_col = char_at_display(
                    self.buf.line(self.cursor_row),
                    target_col,
                    max,
                    self.cfg.tab_width,
                );
                self.update_goal();
                self.focus = Focus::Edit;
                self.message.clear();
            }
            MouseEventKind::ScrollDown => {
                let h = self.text_height();
                self.top = (self.top + 3).min(self.buf.len_lines().saturating_sub(1));
                if self.cursor_row < self.top {
                    self.cursor_row = self.top;
                    self.snap_to_goal();
                }
                let _ = h;
            }
            MouseEventKind::ScrollUp => {
                self.top = self.top.saturating_sub(3);
                if self.cursor_row >= self.top + self.text_height() {
                    self.cursor_row = (self.top + self.text_height()).saturating_sub(1);
                    self.snap_to_goal();
                }
            }
            _ => {}
        }
    }

    // --- input routing --------------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) {
        // Record into macro buffer (before processing, so we capture the key).
        if self.macro_recording {
            self.macro_buf.push(key);
        }
        if self.help_visible {
            match key.code {
                KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    let max = HELP_LINES.len().saturating_sub(self.text_height());
                    self.help_scroll = (self.help_scroll + 1).min(max);
                }
                _ => {
                    self.help_visible = false;
                    self.help_scroll = 0;
                }
            }
            return;
        }
        if matches!(self.focus, Focus::Prompt(_)) {
            self.handle_prompt_key(key);
            return;
        }

        self.message.clear();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(c) if ctrl => self.handle_ctrl(c),
            KeyCode::Char(c) if alt => self.handle_alt(c),
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Tab => self.insert_char('\t'),
            KeyCode::Enter => self.insert_newline(),
            KeyCode::Backspace => self.delete_back(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Home => self.move_home(),
            KeyCode::End => self.move_end(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            _ => {}
        }
    }

    fn handle_ctrl(&mut self, c: char) {
        match c.to_ascii_lowercase() {
            'x' => self.request_quit(),
            'o' => self.prompt_save(false),
            's' => self.quick_save(false),
            'g' => self.help_visible = true,
            'k' => self.cut_line(),
            'u' => self.paste(),
            'a' => self.move_home(),
            'e' => self.move_end(),
            'w' => self.open_search(),
            'z' => self.undo(),
            'y' => self.redo(),
            'n' => {
                self.cfg.line_numbers = !self.cfg.line_numbers;
                self.message = if self.cfg.line_numbers {
                    "Line numbers on".to_string()
                } else {
                    "Line numbers off".to_string()
                };
            }
            'p' => {
                self.cfg.soft_wrap = !self.cfg.soft_wrap;
                self.left = 0; // reset hscroll when toggling wrap
                self.message = if self.cfg.soft_wrap {
                    "Soft wrap on".to_string()
                } else {
                    "Soft wrap off".to_string()
                };
            }
            'c' => {
                self.message = format!(
                    "line {} of {}, col {}",
                    self.cursor_row + 1,
                    self.buf.len_lines(),
                    self.cursor_display_col() + 1
                );
            }
            'r' => self.macro_toggle_record(),
            _ => {}
        }
    }

    fn handle_alt(&mut self, c: char) {
        match c.to_ascii_lowercase() {
            'w' => match &self.kill {
                Some(text) => {
                    if crate::clipboard::write(text) {
                        self.message = "Copied to clipboard".to_string();
                    } else {
                        self.message = "Clipboard unavailable (install xclip or xsel)".to_string();
                    }
                }
                None => self.message = "Nothing to copy (use Ctrl+K first)".to_string(),
            },
            'u' => match crate::clipboard::read() {
                Some(text) if !text.is_empty() => {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    self.kill = Some(text);
                    self.paste();
                    self.message = "Pasted from clipboard".to_string();
                }
                _ => {
                    self.message = "Clipboard empty or unavailable".to_string();
                }
            },
            'r' => self.macro_play(),
            'n' => self.next_buffer(),
            'b' => self.prev_buffer(),
            _ => {}
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let is_search = matches!(
            self.focus,
            Focus::Prompt(Prompt {
                kind: PromptKind::Search { .. },
                ..
            })
        );
        if is_search {
            self.handle_search_key(key);
            return;
        }

        let mut prompt = match std::mem::replace(&mut self.focus, Focus::Edit) {
            Focus::Prompt(p) => p,
            Focus::Edit => return,
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match prompt.kind {
            PromptKind::ConfirmQuit => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.quick_save(true),
                KeyCode::Char('n') | KeyCode::Char('N') => self.should_quit = true,
                KeyCode::Esc => self.message = "Cancelled".to_string(),
                KeyCode::Char('c') if ctrl => self.message = "Cancelled".to_string(),
                _ => self.focus = Focus::Prompt(prompt),
            },
            PromptKind::SaveAs { quit_after } => match key.code {
                KeyCode::Enter => {
                    let name = prompt.input.trim().to_string();
                    if name.is_empty() {
                        self.message = "Cancelled (no file name)".to_string();
                    } else {
                        let path = PathBuf::from(&name);
                        self.do_save(&path, quit_after);
                    }
                }
                KeyCode::Esc => self.message = "Cancelled".to_string(),
                KeyCode::Char('c') if ctrl => self.message = "Cancelled".to_string(),
                KeyCode::Backspace => {
                    prompt.input.pop();
                    self.focus = Focus::Prompt(prompt);
                }
                KeyCode::Char(c) if !ctrl => {
                    prompt.input.push(c);
                    self.focus = Focus::Prompt(prompt);
                }
                _ => self.focus = Focus::Prompt(prompt),
            },
            PromptKind::Search { .. } => unreachable!("handled above"),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Enter => self.search_close(true),
            KeyCode::Esc => self.search_close(false),
            KeyCode::Char('c') if ctrl => self.search_close(false),
            KeyCode::Char('w') if ctrl => self.search_step(true),
            KeyCode::Char('q') if ctrl => self.search_step(false),
            KeyCode::Char('c') if alt => self.search_edit(|input, ci| {
                *ci = !*ci;
                input.clone()
            }),
            KeyCode::Backspace => self.search_edit(|input, _| {
                input.pop();
                input.clone()
            }),
            KeyCode::Char(c) if !ctrl && !alt => self.search_edit(move |input, _| {
                input.push(c);
                input.clone()
            }),
            _ => {}
        }
    }

    // --- rendering ------------------------------------------------------------

    fn scroll(&mut self) {
        let h = self.text_height();
        let tw = self.text_width();

        if self.cfg.soft_wrap {
            // In soft-wrap mode, scroll so the cursor's logical row is visible.
            // We count visual rows upward from top until we would exceed h.
            // Simple heuristic: keep cursor_row in [top, top + h).
            // Full visual-row accounting is deferred to a later iteration.
            if self.cursor_row < self.top {
                self.top = self.cursor_row;
            } else if self.cursor_row >= self.top + h {
                self.top = self.cursor_row + 1 - h;
            }
            self.left = 0;
        } else {
            if self.cursor_row < self.top {
                self.top = self.cursor_row;
            } else if self.cursor_row >= self.top + h {
                self.top = self.cursor_row + 1 - h;
            }
            let dcol = self.cursor_display_col();
            if dcol < self.left {
                self.left = dcol;
            } else if dcol >= self.left + tw {
                self.left = dcol + 1 - tw;
            }
        }
    }

    fn render(&self, out: &mut impl Write) -> io::Result<()> {
        queue!(out, Hide, MoveTo(0, 0))?;

        if self.help_visible {
            return self.render_help(out);
        }

        // Title bar.
        let name = self
            .buf
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "New Buffer".to_string());
        let modified = if self.buf.dirty { " *" } else { "" };
        let ro = if self.buf.readonly { " [readonly]" } else { "" };
        let hl_off = if self.highlight_disabled && !self.syntax_rules.is_empty() {
            " [no hl]"
        } else {
            ""
        };
        let title = format!(" rnano  {name}{modified}{ro}{hl_off}");
        queue!(
            out,
            SetBackgroundColor(self.theme.title_bg),
            SetForegroundColor(Color::White),
            Print(fit(&title, self.width)),
            SetAttribute(Attribute::Reset)
        )?;

        // Text area.
        let h = self.text_height();
        let gutter = self.gutter_width();
        if self.cfg.soft_wrap {
            self.render_text_area_wrapped(out, h, gutter)?;
        } else {
            self.render_text_area_normal(out, h, gutter)?;
        }

        // Status / prompt line.
        let status_row = (self.height - 2) as u16;
        queue!(out, MoveTo(0, status_row))?;
        let status = match &self.focus {
            Focus::Prompt(p) => prompt_text(p),
            Focus::Edit => {
                if self.message.is_empty() {
                    let bufs = if self.buf_total > 1 {
                        format!("  [{}/{}]", self.buf_idx + 1, self.buf_total)
                    } else {
                        String::new()
                    };
                    let rec = if self.macro_recording { "  [REC]" } else { "" };
                    format!(
                        " Ln {}, Col {}   {}{}{}",
                        self.cursor_row + 1,
                        self.cursor_display_col() + 1,
                        self.buf.eol.label(),
                        bufs,
                        rec,
                    )
                } else {
                    format!(" {}", self.message)
                }
            }
        };
        queue!(
            out,
            SetBackgroundColor(self.theme.status_bg),
            SetForegroundColor(Color::White),
            Print(fit(&status, self.width)),
            SetAttribute(Attribute::Reset)
        )?;

        // Key-hint line.
        let help_row = (self.height - 1) as u16;
        queue!(out, MoveTo(0, help_row), Print(fit(HINT_LINE, self.width)))?;

        // Place the cursor.
        match &self.focus {
            Focus::Prompt(p) => {
                let col = UnicodeWidthStr::width(prompt_text(p).as_str())
                    .min(self.width.saturating_sub(1));
                queue!(out, MoveTo(col as u16, status_row), Show)?;
            }
            Focus::Edit => {
                let screen_row = (1 + (self.cursor_row - self.top)) as u16;
                let screen_col = (self.cursor_display_col() - self.left + gutter) as u16;
                queue!(out, MoveTo(screen_col, screen_row), Show)?;
            }
        }

        out.flush()
    }

    fn render_text_area_normal(
        &self,
        out: &mut impl Write,
        h: usize,
        gutter: usize,
    ) -> io::Result<()> {
        for i in 0..h {
            let row = self.top + i;
            queue!(out, MoveTo(0, (1 + i) as u16))?;
            if row < self.buf.len_lines() {
                self.render_gutter_cell(row, gutter, out)?;
                self.render_row_to(row, out)?;
            } else {
                queue!(out, Print(fit("", self.width)))?;
            }
        }
        Ok(())
    }

    /// Soft-wrap rendering: each logical row may occupy multiple screen rows.
    fn render_text_area_wrapped(
        &self,
        out: &mut impl Write,
        h: usize,
        gutter: usize,
    ) -> io::Result<()> {
        let tw = self.text_width();
        let mut screen_row = 0usize;
        let mut log_row = self.top;

        while screen_row < h && log_row < self.buf.len_lines() {
            let line_cols = display_col(
                self.buf.line(log_row),
                self.buf.line_len_chars(log_row),
                self.cfg.tab_width,
            );
            let vrows = ((line_cols + 1) / tw.max(1)).max(1);

            for vr in 0..vrows {
                if screen_row >= h {
                    break;
                }
                queue!(out, MoveTo(0, (1 + screen_row) as u16))?;
                if vr == 0 {
                    self.render_gutter_cell(log_row, gutter, out)?;
                } else if gutter > 0 {
                    let blank = " ".repeat(gutter);
                    queue!(
                        out,
                        SetForegroundColor(self.theme.gutter_fg),
                        Print(&blank),
                        SetAttribute(Attribute::Reset)
                    )?;
                }
                self.render_wrapped_subrow(log_row, vr, tw, out)?;
                screen_row += 1;
            }
            log_row += 1;
        }

        while screen_row < h {
            queue!(
                out,
                MoveTo(0, (1 + screen_row) as u16),
                Print(fit("", self.width))
            )?;
            screen_row += 1;
        }
        Ok(())
    }

    fn render_gutter_cell(
        &self,
        row: usize,
        gutter: usize,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if gutter == 0 {
            return Ok(());
        }
        let digits = gutter - 1;
        let num = format!("{:>width$} ", row + 1, width = digits);
        queue!(
            out,
            SetForegroundColor(self.theme.gutter_fg),
            Print(num),
            SetAttribute(Attribute::Reset)
        )
    }

    fn render_row_to(&self, row: usize, out: &mut impl Write) -> io::Result<()> {
        let search_ranges = self.highlight_screen_ranges(row);
        let syntax_spans = self.syntax_spans_for_row(row);

        if search_ranges.is_empty() && syntax_spans.is_empty() {
            queue!(out, Print(self.render_line(row)))
        } else {
            self.emit_highlighted_line(row, &search_ranges, &syntax_spans, out)
        }
    }

    /// Render the `subrow`-th wrapped segment of a logical row.
    fn render_wrapped_subrow(
        &self,
        row: usize,
        subrow: usize,
        tw: usize,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let start_col = subrow * tw;
        let end_col = start_col + tw;
        // Build a sub-render: same as render_line but with left=start_col and width=tw.
        let line = self.buf.line(row);
        let max = self.buf.line_len_chars(row);
        let tab_width = self.cfg.tab_width;
        let mut rendered = String::new();
        let mut col = 0usize;
        let mut shown = 0usize;
        let mut chars = line.chars();
        for _ in 0..max {
            let ch = chars.next().unwrap();
            let w = char_display_width(ch, col, tab_width);
            if col + w <= start_col {
                col += w;
                continue;
            }
            if col >= end_col {
                break;
            }
            if ch == '\t' || col < start_col || col + w > end_col {
                for c in col..col + w {
                    if c >= start_col && c < end_col {
                        rendered.push(' ');
                        shown += 1;
                    }
                }
            } else {
                rendered.push(ch);
                shown += w;
            }
            col += w;
        }
        while shown < tw {
            rendered.push(' ');
            shown += 1;
        }
        queue!(out, Print(rendered))
    }

    fn highlight_screen_ranges(&self, row: usize) -> Vec<(usize, usize)> {
        let Focus::Prompt(Prompt {
            kind:
                PromptKind::Search {
                    ref matches,
                    current: _,
                    ..
                },
            ..
        }) = self.focus
        else {
            return vec![];
        };
        let tw = self.cfg.tab_width;
        let tw_text = self.text_width();
        let line = self.buf.line(row);
        let mut result = Vec::new();
        for &(mr, cs, ce) in matches {
            if mr != row {
                continue;
            }
            let ds = display_col(line, cs, tw);
            let de = display_col(line, ce, tw);
            if de <= self.left || ds >= self.left + tw_text {
                continue;
            }
            let scr_start = ds.saturating_sub(self.left).min(tw_text);
            let scr_end = de.saturating_sub(self.left).min(tw_text);
            if scr_start < scr_end {
                result.push((scr_start, scr_end));
            }
        }
        merge_ranges(result)
    }

    fn syntax_spans_for_row(&self, row: usize) -> Vec<(usize, usize, Color)> {
        if self.highlight_disabled || self.syntax_rules.is_empty() {
            return vec![];
        }
        let line_str: String = self.buf.line(row).chars().filter(|&c| c != '\n').collect();
        highlight::highlight_line(&line_str, self.syntax_rules)
    }

    fn emit_highlighted_line(
        &self,
        row: usize,
        search_ranges: &[(usize, usize)],
        syntax_spans: &[(usize, usize, Color)],
        out: &mut impl Write,
    ) -> io::Result<()> {
        let rendered: Vec<char> = self.render_line(row).chars().collect();
        let n = rendered.len();

        // Build a per-screen-column color map.
        // Search highlights (#15) override syntax (#27).
        // Screen column = char index into `rendered`.
        let mut fg_map: Vec<Option<Color>> = vec![None; n];
        let mut bg_map: Vec<Option<Color>> = vec![None; n];

        // Apply syntax spans first (converted from char-index to screen-col).
        for &(cs, ce, color) in syntax_spans {
            let line = self.buf.line(row);
            let ds = display_col(line, cs, self.cfg.tab_width);
            let de = display_col(line, ce, self.cfg.tab_width);
            let ss = ds.saturating_sub(self.left).min(n);
            let se = de.saturating_sub(self.left).min(n);
            fg_map[ss..se].fill(Some(color));
        }
        // Apply search highlights (override).
        for &(ss, se) in search_ranges {
            let end = se.min(n);
            bg_map[ss..end].fill(Some(self.theme.search_hl_bg));
            fg_map[ss..end].fill(Some(self.theme.search_hl_fg));
        }

        // Emit runs of characters with the same color.
        let mut i = 0;
        while i < n {
            let cur_fg = fg_map[i];
            let cur_bg = bg_map[i];
            let mut j = i + 1;
            while j < n && fg_map[j] == cur_fg && bg_map[j] == cur_bg {
                j += 1;
            }
            let seg: String = rendered[i..j].iter().collect();
            match (cur_fg, cur_bg) {
                (None, None) => queue!(out, Print(seg))?,
                (Some(fg), None) => queue!(
                    out,
                    SetForegroundColor(fg),
                    Print(seg),
                    SetAttribute(Attribute::Reset)
                )?,
                (fg, Some(bg)) => {
                    queue!(out, SetBackgroundColor(bg))?;
                    if let Some(fg) = fg {
                        queue!(out, SetForegroundColor(fg))?;
                    }
                    queue!(out, Print(seg), SetAttribute(Attribute::Reset))?;
                }
            }
            i = j;
        }
        Ok(())
    }

    fn render_line(&self, row: usize) -> String {
        let line = self.buf.line(row);
        let max = self.buf.line_len_chars(row);
        let tab_width = self.cfg.tab_width;
        let tw = self.text_width();
        let target = self.left + tw;
        let mut out = String::new();
        let mut col = 0usize;
        let mut shown = 0usize;
        let mut chars = line.chars();
        for _ in 0..max {
            let ch = chars.next().unwrap();
            let w = char_display_width(ch, col, tab_width);
            if col + w <= self.left {
                col += w;
                continue;
            }
            if col >= target {
                break;
            }
            if ch == '\t' || col < self.left || col + w > target {
                for c in col..col + w {
                    if c >= self.left && c < target {
                        out.push(' ');
                        shown += 1;
                    }
                }
            } else {
                out.push(ch);
                shown += w;
            }
            col += w;
        }
        while shown < tw {
            out.push(' ');
            shown += 1;
        }
        out
    }

    fn render_help(&self, out: &mut impl Write) -> io::Result<()> {
        queue!(out, Clear(ClearType::All), MoveTo(0, 0))?;
        let h = self.text_height() + 3; // use full screen for help
        let start = self.help_scroll;
        for (i, l) in HELP_LINES.iter().skip(start).take(h).enumerate() {
            queue!(out, MoveTo(0, i as u16), Print(fit(l, self.width)))?;
        }
        queue!(out, Hide)?;
        out.flush()
    }
}

// --- apply a history op to the buffer -----------------------------------------

fn apply_op(buf: &mut Buffer, op: &Op) {
    match op {
        Op::Insert { pos, text } => buf.insert(*pos, text),
        Op::Delete { pos, char_len } => buf.remove(*pos..*pos + *char_len),
    }
    // Note: buf.dirty is set by the rope mutations; we don't clear it here.
    // A future "clean marker" in History would let us detect undo-to-clean.
}

// --- syntax helper ------------------------------------------------------------

fn syntax_for_buf(buf: &Buffer, cfg: &Config) -> (&'static [highlight::Rule], bool) {
    let rules = buf
        .path
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .map(highlight::rules_for_extension)
        .unwrap_or(&[]);
    let file_kb = buf
        .path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() / 1024)
        .unwrap_or(0);
    let disabled = file_kb > cfg.highlight_max_kb || !cfg.syntax;
    (rules, disabled)
}

// --- display-width helpers ----------------------------------------------------

fn char_display_width(ch: char, col: usize, tab_width: usize) -> usize {
    if ch == '\t' {
        tab_width - (col % tab_width)
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

fn display_col(line: RopeSlice, char_idx: usize, tab_width: usize) -> usize {
    let mut col = 0;
    for ch in line.chars().take(char_idx) {
        col += char_display_width(ch, col, tab_width);
    }
    col
}

fn char_at_display(line: RopeSlice, target: usize, max_chars: usize, tab_width: usize) -> usize {
    let mut col = 0;
    let mut idx = 0;
    let mut chars = line.chars();
    while idx < max_chars {
        let Some(ch) = chars.next() else { break };
        let w = char_display_width(ch, col, tab_width);
        if col + w > target {
            break;
        }
        col += w;
        idx += 1;
    }
    idx
}

// --- search helpers -----------------------------------------------------------

fn search_matches(
    rope: &ropey::Rope,
    query: &str,
    case_insensitive: bool,
) -> Vec<(usize, usize, usize)> {
    if query.is_empty() {
        return vec![];
    }
    let text = rope.to_string();
    let (haystack, needle) = if case_insensitive {
        (text.to_lowercase(), query.to_lowercase())
    } else {
        (text.clone(), query.to_string())
    };
    let query_chars = query.chars().count();
    let mut results = Vec::new();
    let mut byte_start = 0;
    while byte_start < haystack.len() {
        let Some(rel) = haystack[byte_start..].find(&needle) else {
            break;
        };
        let abs = byte_start + rel;
        let char_idx = haystack[..abs].chars().count();
        let row = rope.char_to_line(char_idx);
        let col = char_idx - rope.line_to_char(row);
        results.push((row, col, col + query_chars));
        byte_start = abs + needle.len().max(1);
    }
    results
}

fn first_match_at_or_after(
    matches: &[(usize, usize, usize)],
    row: usize,
    col: usize,
) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    matches
        .iter()
        .position(|&(mr, mc, _)| mr > row || (mr == row && mc >= col))
        .or(Some(0))
}

fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if ranges.len() < 2 {
        return ranges;
    }
    ranges.sort_unstable();
    let mut merged = Vec::with_capacity(ranges.len());
    let mut cur = ranges[0];
    for &(s, e) in &ranges[1..] {
        if s <= cur.1 {
            cur.1 = cur.1.max(e);
        } else {
            merged.push(cur);
            cur = (s, e);
        }
    }
    merged.push(cur);
    merged
}

fn prompt_text(p: &Prompt) -> String {
    match &p.kind {
        PromptKind::ConfirmQuit => format!(" {} ", p.label),
        PromptKind::SaveAs { .. } => format!(" {}: {}", p.label, p.input),
        PromptKind::Search {
            case_insensitive,
            matches,
            current,
            ..
        } => {
            let ci = if *case_insensitive { " [i]" } else { "" };
            let count = match current {
                Some(i) => format!(" [{}/{}]", i + 1, matches.len()),
                None if !matches.is_empty() => format!(" [0/{}]", matches.len()),
                _ => String::new(),
            };
            format!(" {}{}:{}{} ", p.label, ci, p.input, count)
        }
    }
}

fn actionable_save_error(path: &Path, e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::PermissionDenied => format!(
            "Permission denied writing \"{}\" — check file permissions or try sudo",
            path.display()
        ),
        io::ErrorKind::StorageFull => format!(
            "No space left on device writing \"{}\" — free up disk space and retry",
            path.display()
        ),
        io::ErrorKind::ReadOnlyFilesystem => format!(
            "Read-only filesystem writing \"{}\" — remount rw or save elsewhere",
            path.display()
        ),
        _ => format!("Error writing \"{}\": {}", path.display(), e),
    }
}

fn fit(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    while w < width {
        out.push(' ');
        w += 1;
    }
    out
}

// --- terminal lifecycle + run loop -------------------------------------------

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<TerminalGuard> {
        terminal::enable_raw_mode()?;
        execute!(
            io::stdout(),
            terminal::EnterAlternateScreen,
            terminal::DisableLineWrap,
            event::EnableMouseCapture
        )?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            event::DisableMouseCapture,
            terminal::EnableLineWrap,
            terminal::LeaveAlternateScreen,
            Show
        );
        let _ = terminal::disable_raw_mode();
    }
}

pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen, Show);
        default(info);
    }));
}

/// Enter the alternate screen and run the editor until the user quits.
/// Uses `event::poll` with a short timeout so autosave ticks can fire (#22).
pub fn run(bufs: Vec<Buffer>, cfg: Config) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let (w, h) = terminal::size()?;
    let mut editor = Editor::new(bufs, w as usize, h as usize, cfg);
    let mut out = io::stdout();

    // Poll interval: short enough for autosave ticks, long enough not to spin.
    let poll_ms = if editor.cfg.autosave_interval > 0 {
        500
    } else {
        5_000
    };

    while !editor.should_quit {
        editor.maybe_autosave();
        editor.scroll();
        editor.render(&mut out)?;

        if event::poll(Duration::from_millis(poll_ms))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => editor.handle_key(key),
                Event::Mouse(me) => editor.handle_mouse(me),
                Event::Resize(w, h) => {
                    editor.width = (w as usize).max(1);
                    editor.height = (h as usize).max(4);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// --- tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn editor_with(text: &str) -> Editor {
        let mut buf = Buffer::new(false);
        buf.insert(0, text);
        buf.dirty = false;
        Editor::new(vec![buf], 80, 24, Config::default())
    }

    #[test]
    fn insert_and_newline_split() {
        let mut e = editor_with("ab");
        e.cursor_col = 1;
        e.insert_char('X');
        assert_eq!(e.buf.rope.to_string(), "aXb");
        e.insert_newline();
        assert_eq!(e.buf.rope.to_string(), "aX\nb");
    }

    #[test]
    fn backspace_joins_lines() {
        let mut e = editor_with("ab\ncd");
        e.cursor_row = 1;
        e.cursor_col = 0;
        e.delete_back();
        assert_eq!(e.buf.rope.to_string(), "abcd");
        assert_eq!((e.cursor_row, e.cursor_col), (0, 2));
    }

    #[test]
    fn vertical_move_keeps_goal_column() {
        let mut e = editor_with("123456\nab\n123456");
        e.cursor_col = 4;
        e.update_goal();
        e.move_down();
        assert_eq!(e.cursor_col, 2);
        e.move_down();
        assert_eq!(e.cursor_col, 4);
    }

    #[test]
    fn cut_and_paste_line() {
        let mut e = editor_with("one\ntwo\nthree");
        e.cursor_row = 1;
        e.cut_line();
        assert_eq!(e.buf.rope.to_string(), "one\nthree");
        e.cursor_row = 1;
        e.cursor_col = e.buf.line_len_chars(e.cursor_row);
        e.paste();
        assert!(e.buf.rope.to_string().contains("two\n"));
    }

    #[test]
    fn tab_display_width_alignment() {
        let mut b = Buffer::new(false);
        b.insert(0, "\tx");
        let line = b.line(0);
        let tw = Config::default().tab_width;
        assert_eq!(display_col(line, 0, tw), 0);
        assert_eq!(display_col(line, 1, tw), tw);
    }

    #[test]
    fn readonly_blocks_edits() {
        let mut buf = Buffer::new(true);
        buf.insert(0, "data");
        buf.dirty = false;
        let mut e = Editor::new(vec![buf], 80, 24, Config::default());
        e.insert_char('Z');
        assert_eq!(e.buf.rope.to_string(), "data");
    }

    // --- undo/redo (#16) ------------------------------------------------------

    #[test]
    fn undo_single_char() {
        let mut e = editor_with("ab");
        e.cursor_col = 2;
        e.insert_char('c');
        assert_eq!(e.buf.rope.to_string(), "abc");
        e.undo();
        assert_eq!(e.buf.rope.to_string(), "ab");
        assert_eq!(e.cursor_col, 2);
    }

    #[test]
    fn undo_squashes_consecutive_inserts() {
        let mut e = editor_with("");
        e.insert_char('a');
        e.insert_char('b');
        e.insert_char('c');
        assert_eq!(
            e.history.undo.len(),
            1,
            "three consecutive inserts should squash into one"
        );
        e.undo();
        assert_eq!(e.buf.rope.to_string(), "");
    }

    #[test]
    fn redo_after_undo() {
        let mut e = editor_with("");
        e.insert_char('x');
        e.undo();
        assert_eq!(e.buf.rope.to_string(), "");
        e.redo();
        assert_eq!(e.buf.rope.to_string(), "x");
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut e = editor_with("");
        e.insert_char('a');
        e.undo();
        e.insert_char('b'); // new edit after undo
        assert!(!e.history.can_redo());
        assert_eq!(e.buf.rope.to_string(), "b");
    }

    #[test]
    fn undo_delete_back() {
        let mut e = editor_with("abc");
        e.cursor_col = 3;
        e.delete_back();
        assert_eq!(e.buf.rope.to_string(), "ab");
        e.undo();
        assert_eq!(e.buf.rope.to_string(), "abc");
        assert_eq!(e.cursor_col, 3);
    }

    #[test]
    fn undo_nothing_shows_message() {
        let mut e = editor_with("x");
        e.undo();
        assert_eq!(e.message, "Nothing to undo");
    }

    // --- line numbers (#18) ---------------------------------------------------

    #[test]
    fn gutter_width_off_by_default() {
        let e = editor_with("hello");
        assert_eq!(e.gutter_width(), 0);
        assert_eq!(e.text_width(), 80);
    }

    #[test]
    fn gutter_width_when_enabled() {
        let mut e = editor_with("hello\nworld");
        e.cfg.line_numbers = true;
        // 2 lines → 1 digit + 1 space = 2
        assert_eq!(e.gutter_width(), 2);
        assert_eq!(e.text_width(), 78);
    }

    // --- search (#15) ---------------------------------------------------------

    #[test]
    fn search_matches_basic() {
        use ropey::Rope;
        let rope = Rope::from_str("hello world\nhello again");
        let m = search_matches(&rope, "hello", false);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn search_next_wraps() {
        let mut e = editor_with("ab ab ab");
        e.open_search();
        for ch in "ab".chars() {
            e.handle_search_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        e.handle_search_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(e.cursor_col, 3);
        e.handle_search_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(e.cursor_col, 6);
        e.handle_search_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(e.cursor_col, 0);
    }

    #[test]
    fn search_esc_restores_cursor() {
        let mut e = editor_with("hello");
        e.cursor_col = 3;
        e.open_search();
        e.handle_search_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(e.cursor_col, 3);
    }

    // --- multiple buffers (#32) -----------------------------------------------

    fn two_buf_editor(text1: &str, text2: &str) -> Editor {
        let mut b1 = Buffer::new(false);
        b1.insert(0, text1);
        b1.dirty = false;
        let mut b2 = Buffer::new(false);
        b2.insert(0, text2);
        b2.dirty = false;
        Editor::new(vec![b1, b2], 80, 24, Config::default())
    }

    #[test]
    fn multi_buf_next_switches_content() {
        let mut e = two_buf_editor("alpha", "beta");
        assert_eq!(e.buf.rope.to_string(), "alpha");
        e.next_buffer();
        assert_eq!(e.buf.rope.to_string(), "beta");
        assert_eq!(e.buf_idx, 1);
        assert_eq!(e.buf_total, 2);
    }

    #[test]
    fn multi_buf_prev_wraps() {
        let mut e = two_buf_editor("alpha", "beta");
        e.prev_buffer();
        // wraps: should now show "beta"
        assert_eq!(e.buf.rope.to_string(), "beta");
    }

    #[test]
    fn multi_buf_round_trip() {
        let mut e = two_buf_editor("one", "two");
        e.next_buffer();
        e.next_buffer(); // wraps back to first
        assert_eq!(e.buf.rope.to_string(), "one");
        assert_eq!(e.buf_idx, 0);
    }

    #[test]
    fn single_buf_next_shows_message() {
        let mut e = editor_with("solo");
        e.next_buffer();
        assert_eq!(e.message, "Only one buffer open");
    }

    // --- macros (#33) ---------------------------------------------------------

    #[test]
    fn macro_record_and_play() {
        let mut e = editor_with("");
        // Start recording
        e.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert!(e.macro_recording);
        // Type "ab"
        e.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        e.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        // Stop recording (Ctrl+R again)
        e.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert!(!e.macro_recording);
        assert_eq!(e.buf.rope.to_string(), "ab");
        // Move cursor and play
        e.cursor_col = 2;
        e.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
        assert_eq!(e.buf.rope.to_string(), "abab");
    }

    #[test]
    fn macro_play_empty_shows_message() {
        let mut e = editor_with("x");
        e.macro_play();
        assert!(e.message.contains("No macro"));
    }

    // --- misc helpers ---------------------------------------------------------

    #[test]
    fn merge_ranges_overlapping() {
        let r = merge_ranges(vec![(2, 5), (0, 3), (4, 7)]);
        assert_eq!(r, vec![(0, 7)]);
    }
}
