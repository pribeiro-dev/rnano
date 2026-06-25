//! Terminal editor: viewport, cursor, input handling, and the render loop.
//!
//! Screen layout (top to bottom):
//! ```text
//!   row 0           title bar (reverse video)
//!   rows 1..H-2     text area
//!   row H-2         status / prompt line
//!   row H-1         key-hint line (reverse video)
//! ```

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};
use ropey::RopeSlice;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::buffer::Buffer;

/// Display width of a tab stop.
const TAB_WIDTH: usize = 4;

/// What input is currently directed at.
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
    /// Asking for a path to write to; optionally quit once written.
    SaveAs { quit_after: bool },
    /// Asking whether to save a modified buffer before quitting.
    ConfirmQuit,
}

pub struct Editor {
    buf: Buffer,
    cursor_row: usize,
    cursor_col: usize, // char index within the line (excludes the newline)
    goal_col: usize,   // display column remembered across vertical moves
    top: usize,        // first visible buffer line
    left: usize,       // horizontal scroll, in display columns
    width: usize,
    height: usize,
    message: String,
    focus: Focus,
    help_visible: bool,
    should_quit: bool,
    kill: Option<String>, // last cut line, for paste
    backup: bool,
}

impl Editor {
    pub fn new(buf: Buffer, width: usize, height: usize, backup: bool) -> Editor {
        let message = if buf.is_new {
            "New file".to_string()
        } else if buf.readonly {
            "Read-only mode".to_string()
        } else {
            String::new()
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
            should_quit: false,
            kill: None,
            backup,
        }
    }

    fn text_height(&self) -> usize {
        self.height.saturating_sub(3).max(1)
    }

    // --- cursor / movement ----------------------------------------------------

    fn cur_idx(&self) -> usize {
        self.buf.char_idx(self.cursor_row, self.cursor_col)
    }

    fn cursor_display_col(&self) -> usize {
        display_col(self.buf.line(self.cursor_row), self.cursor_col)
    }

    fn update_goal(&mut self) {
        self.goal_col = self.cursor_display_col();
    }

    fn snap_to_goal(&mut self) {
        let max = self.buf.line_len_chars(self.cursor_row);
        self.cursor_col = char_at_display(self.buf.line(self.cursor_row), self.goal_col, max);
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
        let idx = self.cur_idx();
        self.buf.insert_char(idx, ch);
        self.cursor_col += 1;
        self.update_goal();
    }

    fn insert_newline(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let idx = self.cur_idx();
        self.buf.insert_char(idx, '\n');
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.update_goal();
    }

    fn delete_back(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        if self.cursor_col > 0 {
            let idx = self.cur_idx();
            self.buf.remove(idx - 1..idx);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            let prev_len = self.buf.line_len_chars(self.cursor_row - 1);
            let idx = self.cur_idx();
            self.buf.remove(idx - 1..idx); // remove the joining newline
            self.cursor_row -= 1;
            self.cursor_col = prev_len;
        }
        self.update_goal();
    }

    fn delete_forward(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let line_len = self.buf.line_len_chars(self.cursor_row);
        let idx = self.cur_idx();
        if self.cursor_col < line_len {
            self.buf.remove(idx..idx + 1);
        } else if self.cursor_row + 1 < self.buf.len_lines() {
            self.buf.remove(idx..idx + 1); // join with next line
        }
        self.update_goal();
    }

    fn cut_line(&mut self) {
        if !self.ensure_writable() {
            return;
        }
        let row = self.cursor_row;
        let start = self.buf.rope.line_to_char(row);
        let end = if row + 1 < self.buf.len_lines() {
            self.buf.rope.line_to_char(row + 1)
        } else {
            self.buf.rope.len_chars()
        };
        self.kill = Some(self.buf.rope.slice(start..end).to_string());
        self.buf.remove(start..end);
        if self.cursor_row >= self.buf.len_lines() {
            self.cursor_row = self.buf.len_lines().saturating_sub(1);
        }
        self.cursor_col = self
            .cursor_col
            .min(self.buf.line_len_chars(self.cursor_row));
        self.update_goal();
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
        let idx = self.cur_idx();
        self.buf.insert(idx, &text);
        let new_idx = idx + text.chars().count();
        self.cursor_row = self.buf.rope.char_to_line(new_idx);
        self.cursor_col = new_idx - self.buf.rope.line_to_char(self.cursor_row);
        self.update_goal();
        self.message = "Uncut text".to_string();
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
        match self.buf.save(path, self.backup) {
            Ok(()) => {
                self.message = format!("Wrote \"{}\"", path.display());
                if quit_after {
                    self.should_quit = true;
                }
            }
            Err(e) => {
                self.message = format!("Error writing \"{}\": {}", path.display(), e);
            }
        }
    }

    // --- input routing --------------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) {
        if self.help_visible {
            self.help_visible = false;
            return;
        }
        if matches!(self.focus, Focus::Prompt(_)) {
            self.handle_prompt_key(key);
            return;
        }

        self.message.clear();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(c) if ctrl => self.handle_ctrl(c),
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
            'c' => {
                self.message = format!(
                    "line {} of {}, col {}",
                    self.cursor_row + 1,
                    self.buf.len_lines(),
                    self.cursor_display_col() + 1
                );
            }
            _ => {}
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
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
                _ => self.focus = Focus::Prompt(prompt), // ignore, keep asking
            },
            PromptKind::SaveAs { quit_after } => match key.code {
                KeyCode::Enter => {
                    let name = prompt.input.trim();
                    if name.is_empty() {
                        self.message = "Cancelled (no file name)".to_string();
                    } else {
                        let path = PathBuf::from(name);
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
        }
    }

    // --- rendering ------------------------------------------------------------

    fn scroll(&mut self) {
        let h = self.text_height();
        if self.cursor_row < self.top {
            self.top = self.cursor_row;
        } else if self.cursor_row >= self.top + h {
            self.top = self.cursor_row + 1 - h;
        }
        let dcol = self.cursor_display_col();
        if dcol < self.left {
            self.left = dcol;
        } else if dcol >= self.left + self.width {
            self.left = dcol + 1 - self.width;
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
        let title = format!(" rnano  {name}{modified}{ro}");
        queue!(
            out,
            SetAttribute(Attribute::Reverse),
            Print(fit(&title, self.width)),
            SetAttribute(Attribute::Reset)
        )?;

        // Text area.
        let h = self.text_height();
        for i in 0..h {
            let row = self.top + i;
            queue!(out, MoveTo(0, (1 + i) as u16))?;
            if row < self.buf.len_lines() {
                queue!(out, Print(self.render_line(row)))?;
            } else {
                queue!(out, Print(fit("", self.width)))?;
            }
        }

        // Status / prompt line.
        let status_row = (self.height - 2) as u16;
        queue!(out, MoveTo(0, status_row))?;
        let status = match &self.focus {
            Focus::Prompt(p) => prompt_text(p),
            Focus::Edit => {
                if self.message.is_empty() {
                    format!(
                        " Ln {}, Col {}   {}",
                        self.cursor_row + 1,
                        self.cursor_display_col() + 1,
                        self.buf.eol.label(),
                    )
                } else {
                    format!(" {}", self.message)
                }
            }
        };
        queue!(
            out,
            SetAttribute(Attribute::Reverse),
            Print(fit(&status, self.width)),
            SetAttribute(Attribute::Reset)
        )?;

        // Key-hint line.
        let help_row = (self.height - 1) as u16;
        let hints = " ^O Write  ^X Exit  ^G Help  ^K Cut  ^U Paste  ^A Home  ^E End";
        queue!(out, MoveTo(0, help_row), Print(fit(hints, self.width)))?;

        // Place the cursor.
        match &self.focus {
            Focus::Prompt(p) => {
                let col = UnicodeWidthStr::width(prompt_text(p).as_str())
                    .min(self.width.saturating_sub(1));
                queue!(out, MoveTo(col as u16, status_row), Show)?;
            }
            Focus::Edit => {
                let screen_row = (1 + (self.cursor_row - self.top)) as u16;
                let screen_col = (self.cursor_display_col() - self.left) as u16;
                queue!(out, MoveTo(screen_col, screen_row), Show)?;
            }
        }

        out.flush()
    }

    /// Build the visible portion of `row`, honoring tabs, wide chars, and the
    /// horizontal scroll offset, padded to the full screen width.
    fn render_line(&self, row: usize) -> String {
        let line = self.buf.line(row);
        let max = self.buf.line_len_chars(row);
        let target = self.left + self.width;
        let mut out = String::new();
        let mut col = 0; // display column from the start of the line
        let mut shown = 0; // display cells emitted into `out`
        let mut chars = line.chars();
        for _ in 0..max {
            let ch = chars.next().unwrap();
            let w = char_display_width(ch, col);
            if col + w <= self.left {
                col += w; // entirely left of the viewport
                continue;
            }
            if col >= target {
                break;
            }
            if ch == '\t' || col < self.left || col + w > target {
                // Expand tabs, and substitute spaces for wide chars that
                // straddle a scroll edge, to keep columns aligned.
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
        while shown < self.width {
            out.push(' ');
            shown += 1;
        }
        out
    }

    fn render_help(&self, out: &mut impl Write) -> io::Result<()> {
        queue!(out, Clear(ClearType::All), MoveTo(0, 0))?;
        let lines = [
            "rnano — help",
            "",
            "  Arrows / Home / End / PgUp / PgDn   Move",
            "  Ctrl+A / Ctrl+E                     Start / end of line",
            "  Ctrl+O                              Write file (save as)",
            "  Ctrl+S                              Save to current file",
            "  Ctrl+K / Ctrl+U                     Cut line / paste",
            "  Ctrl+C                              Show cursor position",
            "  Ctrl+G                              This help",
            "  Ctrl+X                              Exit (prompts if modified)",
            "",
            "  Press any key to return.",
        ];
        for (i, l) in lines.iter().enumerate() {
            queue!(out, MoveTo(0, i as u16), Print(fit(l, self.width)))?;
        }
        queue!(out, Hide)?;
        out.flush()
    }
}

// --- display-width helpers ----------------------------------------------------

fn char_display_width(ch: char, col: usize) -> usize {
    if ch == '\t' {
        TAB_WIDTH - (col % TAB_WIDTH)
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

/// Display column at which the char `char_idx` of `line` begins.
fn display_col(line: RopeSlice, char_idx: usize) -> usize {
    let mut col = 0;
    for ch in line.chars().take(char_idx) {
        col += char_display_width(ch, col);
    }
    col
}

/// Char index in `line` whose start is nearest to (but not past) display column
/// `target`, clamped to `max_chars`.
fn char_at_display(line: RopeSlice, target: usize, max_chars: usize) -> usize {
    let mut col = 0;
    let mut idx = 0;
    let mut chars = line.chars();
    while idx < max_chars {
        let Some(ch) = chars.next() else { break };
        let w = char_display_width(ch, col);
        if col + w > target {
            break;
        }
        col += w;
        idx += 1;
    }
    idx
}

fn prompt_text(p: &Prompt) -> String {
    match p.kind {
        PromptKind::ConfirmQuit => format!(" {} ", p.label),
        PromptKind::SaveAs { .. } => format!(" {}: {}", p.label, p.input),
    }
}

/// Truncate or pad `s` to exactly `width` display columns.
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
            terminal::DisableLineWrap
        )?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            terminal::EnableLineWrap,
            terminal::LeaveAlternateScreen,
            Show
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// Restore the terminal even if the editor panics.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen, Show);
        default(info);
    }));
}

/// Enter the alternate screen and run the editor until the user quits.
pub fn run(buf: Buffer, backup: bool) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let (w, h) = terminal::size()?;
    let mut editor = Editor::new(buf, w as usize, h as usize, backup);
    let mut out = io::stdout();

    while !editor.should_quit {
        editor.scroll();
        editor.render(&mut out)?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => editor.handle_key(key),
            Event::Resize(w, h) => {
                editor.width = (w as usize).max(1);
                editor.height = (h as usize).max(4);
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with(text: &str) -> Editor {
        let mut buf = Buffer::new(false);
        buf.insert(0, text);
        buf.dirty = false;
        Editor::new(buf, 80, 24, false)
    }

    #[test]
    fn insert_and_newline_split() {
        let mut e = editor_with("ab");
        e.cursor_col = 1;
        e.insert_char('X');
        assert_eq!(e.buf.rope.to_string(), "aXb");
        assert_eq!((e.cursor_row, e.cursor_col), (0, 2));
        e.insert_newline();
        assert_eq!(e.buf.rope.to_string(), "aX\nb");
        assert_eq!((e.cursor_row, e.cursor_col), (1, 0));
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
        // Move to col 4 on a long line, up over a short line, back down.
        let mut e = editor_with("123456\nab\n123456");
        e.cursor_row = 0;
        e.cursor_col = 4;
        e.update_goal();
        e.move_down(); // onto "ab" (len 2) -> clamps to col 2
        assert_eq!((e.cursor_row, e.cursor_col), (1, 2));
        e.move_down(); // onto "123456" -> restores goal col 4
        assert_eq!((e.cursor_row, e.cursor_col), (2, 4));
    }

    #[test]
    fn cut_and_paste_line() {
        let mut e = editor_with("one\ntwo\nthree");
        e.cursor_row = 1;
        e.cursor_col = 0;
        e.cut_line();
        assert_eq!(e.buf.rope.to_string(), "one\nthree");
        e.cursor_row = 2.min(e.buf.len_lines() - 1);
        e.cursor_row = 1; // back onto "three"... place at end
        e.cursor_col = e.buf.line_len_chars(e.cursor_row);
        e.paste();
        assert!(e.buf.rope.to_string().contains("two\n"));
    }

    #[test]
    fn tab_display_width_alignment() {
        // A leading tab advances to the next multiple of TAB_WIDTH.
        let buf = {
            let mut b = Buffer::new(false);
            b.insert(0, "\tx");
            b
        };
        let line = buf.line(0);
        assert_eq!(display_col(line, 0), 0);
        assert_eq!(display_col(line, 1), TAB_WIDTH); // 'x' starts at the tab stop
    }

    #[test]
    fn readonly_blocks_edits() {
        let mut buf = Buffer::new(true);
        buf.insert(0, "data");
        buf.dirty = false;
        let mut e = Editor::new(buf, 80, 24, false);
        e.insert_char('Z');
        assert_eq!(e.buf.rope.to_string(), "data");
        assert!(!e.buf.dirty);
    }
}
