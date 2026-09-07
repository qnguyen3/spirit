use pathfinder_color::ColorU;

use super::{cursor_to_ansi, grid_to_ansi, modes_to_ansi};
use crate::model::ansi::{Color, NamedColor};
use crate::model::grid::cell::Flags;
use crate::model::grid::row::Row;
use crate::model::mode::TermMode;

fn row_from(text: &str, columns: usize) -> Row {
    let mut row = Row::new(columns);
    for (index, character) in text.chars().take(columns).enumerate() {
        row[index].c = character;
    }
    row.occ = text.chars().count().min(columns);
    row
}

#[test]
fn plain_text_round_trips_without_escapes() {
    let row = row_from("hello", 10);
    assert_eq!(grid_to_ansi(&[&row], 10), "hello");
}

#[test]
fn rows_are_separated_by_carriage_return_line_feed() {
    let first = row_from("one", 10);
    let second = row_from("two", 10);
    assert_eq!(grid_to_ansi(&[&first, &second], 10), "one\r\ntwo");
}

#[test]
fn trailing_blank_cells_are_dropped() {
    let mut row = row_from("hi", 10);
    row.occ = 10;
    assert_eq!(grid_to_ansi(&[&row], 10), "hi");
}

#[test]
fn an_empty_row_renders_as_nothing() {
    let row = Row::new(10);
    assert_eq!(grid_to_ansi(&[&row], 10), "");
}

#[test]
fn named_colors_use_the_standard_thirty_and_forty_ranges() {
    let mut row = row_from("ab", 4);
    row[0].fg = Color::Named(NamedColor::Red);
    row[1].fg = Color::Named(NamedColor::Foreground);
    let rendered = grid_to_ansi(&[&row], 4);
    assert!(rendered.starts_with("\u{1b}[31ma"), "{rendered:?}");
    assert!(rendered.contains("39"), "{rendered:?}");
}

#[test]
fn bright_named_colors_use_the_ninety_range() {
    let mut row = row_from("a", 4);
    row[0].fg = Color::Named(NamedColor::BrightCyan);
    assert_eq!(grid_to_ansi(&[&row], 4), "\u{1b}[96ma\u{1b}[0m");
}

#[test]
fn background_colors_use_the_forty_range() {
    let mut row = row_from("a", 4);
    row[0].bg = Color::Named(NamedColor::Blue);
    assert_eq!(grid_to_ansi(&[&row], 4), "\u{1b}[44ma\u{1b}[0m");
}

#[test]
fn indexed_colors_use_the_two_fifty_six_form() {
    let mut row = row_from("a", 4);
    row[0].fg = Color::Indexed(200);
    assert_eq!(grid_to_ansi(&[&row], 4), "\u{1b}[38;5;200ma\u{1b}[0m");
}

#[test]
fn true_colors_use_the_rgb_form() {
    let mut row = row_from("a", 4);
    row[0].fg = Color::Spec(ColorU::new(10, 20, 30, 255));
    assert_eq!(grid_to_ansi(&[&row], 4), "\u{1b}[38;2;10;20;30ma\u{1b}[0m");
}

#[test]
fn attributes_map_to_their_sgr_parameters() {
    let mut row = row_from("a", 4);
    row[0].flags = Flags::BOLD | Flags::ITALIC | Flags::UNDERLINE;
    let rendered = grid_to_ansi(&[&row], 4);
    assert!(rendered.starts_with("\u{1b}[1;3;4m"), "{rendered:?}");
}

#[test]
fn turning_an_attribute_off_emits_a_reset_first() {
    let mut row = row_from("ab", 4);
    row[0].flags = Flags::BOLD;
    let rendered = grid_to_ansi(&[&row], 4);
    assert_eq!(rendered, "\u{1b}[1ma\u{1b}[0mb");
}

#[test]
fn an_unchanged_style_is_not_re_emitted() {
    let mut row = row_from("ab", 4);
    row[0].fg = Color::Named(NamedColor::Green);
    row[1].fg = Color::Named(NamedColor::Green);
    assert_eq!(grid_to_ansi(&[&row], 4), "\u{1b}[32mab\u{1b}[0m");
}

#[test]
fn style_carries_across_rows_without_being_restated() {
    let mut first = row_from("a", 4);
    first[0].fg = Color::Named(NamedColor::Green);
    let mut second = row_from("b", 4);
    second[0].fg = Color::Named(NamedColor::Green);
    assert_eq!(
        grid_to_ansi(&[&first, &second], 4),
        "\u{1b}[32ma\r\nb\u{1b}[0m"
    );
}

#[test]
fn wide_char_spacers_are_skipped() {
    let mut row = row_from("ab", 4);
    row[1].flags = Flags::WIDE_CHAR_SPACER;
    assert_eq!(grid_to_ansi(&[&row], 4), "a");
}

#[test]
fn modes_emit_a_set_or_reset_for_every_tracked_flag() {
    let rendered = modes_to_ansi(TermMode::SHOW_CURSOR | TermMode::BRACKETED_PASTE);
    assert!(rendered.contains("\u{1b}[?25h"));
    assert!(rendered.contains("\u{1b}[?2004h"));
    assert!(rendered.contains("\u{1b}[?1l"));
    assert!(rendered.contains("\u{1b}[?1000l"));
}

#[test]
fn modes_are_stable_for_the_same_input() {
    let mode = TermMode::SHOW_CURSOR | TermMode::LINE_WRAP;
    assert_eq!(modes_to_ansi(mode), modes_to_ansi(mode));
}

#[test]
fn cursor_positions_are_one_based() {
    assert_eq!(cursor_to_ansi(0, 0), "\u{1b}[1;1H");
    assert_eq!(cursor_to_ansi(11, 4), "\u{1b}[12;5H");
}
