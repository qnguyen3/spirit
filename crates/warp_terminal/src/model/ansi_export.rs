use crate::model::ansi::{Color, NamedColor};
use crate::model::char_or_str::CharOrStr;
use crate::model::grid::cell::{Cell, Flags};
use crate::model::grid::row::Row;
use crate::model::mode::TermMode;

const RESET: &str = "\u{1b}[0m";

pub fn grid_to_ansi(rows: &[&Row], columns: usize) -> String {
    let mut out = String::new();
    let mut style = CellStyle::default();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push_str("\r\n");
        }
        style = append_row(&mut out, row, columns, style);
    }
    if style != CellStyle::default() {
        out.push_str(RESET);
    }
    out
}

fn append_row(out: &mut String, row: &Row, columns: usize, mut style: CellStyle) -> CellStyle {
    let width = columns.min(row.occ.max(0));
    let mut last_used = 0;
    for column in 0..width {
        if !is_blank(&row[column]) {
            last_used = column + 1;
        }
    }
    for column in 0..last_used {
        let cell = &row[column];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let next = CellStyle::of(cell);
        if next != style {
            out.push_str(&next.transition_from(style));
            style = next;
        }
        match cell.content_for_display() {
            CharOrStr::Char(character) => out.push(character),
            CharOrStr::Str(text) => out.push_str(text),
        }
    }
    style
}

fn is_blank(cell: &Cell) -> bool {
    let content_is_space = matches!(cell.content_for_display(), CharOrStr::Char(' '));
    content_is_space && CellStyle::of(cell) == CellStyle::default()
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    foreground: Color,
    background: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    hidden: bool,
    strikeout: bool,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            foreground: Color::Named(NamedColor::Foreground),
            background: Color::Named(NamedColor::Background),
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
            hidden: false,
            strikeout: false,
        }
    }
}

impl CellStyle {
    fn of(cell: &Cell) -> Self {
        Self {
            foreground: cell.fg,
            background: cell.bg,
            bold: cell.flags.contains(Flags::BOLD),
            dim: cell.flags.contains(Flags::DIM),
            italic: cell.flags.contains(Flags::ITALIC),
            underline: cell.flags.contains(Flags::UNDERLINE),
            inverse: cell.flags.contains(Flags::INVERSE),
            hidden: cell.flags.contains(Flags::HIDDEN),
            strikeout: cell.flags.contains(Flags::STRIKEOUT),
        }
    }

    fn transition_from(self, previous: Self) -> String {
        let turns_something_off = (previous.bold && !self.bold)
            || (previous.dim && !self.dim)
            || (previous.italic && !self.italic)
            || (previous.underline && !self.underline)
            || (previous.inverse && !self.inverse)
            || (previous.hidden && !self.hidden)
            || (previous.strikeout && !self.strikeout);

        let mut parameters: Vec<String> = Vec::new();
        let base = if turns_something_off {
            parameters.push("0".to_owned());
            Self::default()
        } else {
            previous
        };

        if self.bold && !base.bold {
            parameters.push("1".to_owned());
        }
        if self.dim && !base.dim {
            parameters.push("2".to_owned());
        }
        if self.italic && !base.italic {
            parameters.push("3".to_owned());
        }
        if self.underline && !base.underline {
            parameters.push("4".to_owned());
        }
        if self.inverse && !base.inverse {
            parameters.push("7".to_owned());
        }
        if self.hidden && !base.hidden {
            parameters.push("8".to_owned());
        }
        if self.strikeout && !base.strikeout {
            parameters.push("9".to_owned());
        }
        if self.foreground != base.foreground {
            parameters.extend(color_parameters(self.foreground, true));
        }
        if self.background != base.background {
            parameters.extend(color_parameters(self.background, false));
        }

        if parameters.is_empty() {
            String::new()
        } else {
            format!("\u{1b}[{}m", parameters.join(";"))
        }
    }
}

fn color_parameters(color: Color, foreground: bool) -> Vec<String> {
    let default_parameter = if foreground { "39" } else { "49" };
    match color {
        Color::Named(named) => match named_color_index(named) {
            Some(index) if index < 8 => {
                let base = if foreground { 30 } else { 40 };
                vec![(base + index).to_string()]
            }
            Some(index) => {
                let base = if foreground { 90 } else { 100 };
                vec![(base + index - 8).to_string()]
            }
            None => vec![default_parameter.to_owned()],
        },
        Color::Indexed(index) => {
            let selector = if foreground { "38" } else { "48" };
            vec![selector.to_owned(), "5".to_owned(), index.to_string()]
        }
        Color::Spec(spec) => {
            let selector = if foreground { "38" } else { "48" };
            vec![
                selector.to_owned(),
                "2".to_owned(),
                spec.r.to_string(),
                spec.g.to_string(),
                spec.b.to_string(),
            ]
        }
    }
}

fn named_color_index(named: NamedColor) -> Option<u16> {
    match named {
        NamedColor::Black | NamedColor::DimBlack => Some(0),
        NamedColor::Red | NamedColor::DimRed => Some(1),
        NamedColor::Green | NamedColor::DimGreen => Some(2),
        NamedColor::Yellow | NamedColor::DimYellow => Some(3),
        NamedColor::Blue | NamedColor::DimBlue => Some(4),
        NamedColor::Magenta | NamedColor::DimMagenta => Some(5),
        NamedColor::Cyan | NamedColor::DimCyan => Some(6),
        NamedColor::White | NamedColor::DimWhite => Some(7),
        NamedColor::BrightBlack => Some(8),
        NamedColor::BrightRed => Some(9),
        NamedColor::BrightGreen => Some(10),
        NamedColor::BrightYellow => Some(11),
        NamedColor::BrightBlue => Some(12),
        NamedColor::BrightMagenta => Some(13),
        NamedColor::BrightCyan => Some(14),
        NamedColor::BrightWhite => Some(15),
        NamedColor::Foreground
        | NamedColor::Background
        | NamedColor::Cursor
        | NamedColor::BrightForeground
        | NamedColor::DimForeground => None,
    }
}

pub fn modes_to_ansi(mode: TermMode) -> String {
    let mut out = String::new();
    let settings = [
        (TermMode::SHOW_CURSOR, "?25"),
        (TermMode::APP_CURSOR, "?1"),
        (TermMode::APP_KEYPAD, "?66"),
        (TermMode::BRACKETED_PASTE, "?2004"),
        (TermMode::LINE_WRAP, "?7"),
        (TermMode::ORIGIN, "?6"),
        (TermMode::INSERT, "4"),
        (TermMode::MOUSE_REPORT_CLICK, "?1000"),
        (TermMode::MOUSE_DRAG, "?1002"),
        (TermMode::MOUSE_MOTION, "?1003"),
        (TermMode::SGR_MOUSE, "?1006"),
        (TermMode::FOCUS_IN_OUT, "?1004"),
    ];
    for (flag, parameter) in settings {
        let suffix = if mode.contains(flag) { "h" } else { "l" };
        out.push_str(&format!("\u{1b}[{parameter}{suffix}"));
    }
    out
}

pub fn cursor_to_ansi(row: usize, column: usize) -> String {
    format!("\u{1b}[{};{}H", row + 1, column + 1)
}

#[cfg(test)]
#[path = "ansi_export_tests.rs"]
mod tests;
