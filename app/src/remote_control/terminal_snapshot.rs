use base64::Engine as _;
use remote_control::limits::HISTORY_SNAPSHOT_MAX_BYTES;
use remote_control::protocol::TerminalMode;
use warp_terminal::model::TermMode;
use warp_terminal::model::ansi_export::{cursor_to_ansi, grid_to_ansi, modes_to_ansi};
use warp_terminal::model::grid::Dimensions as _;
use warp_terminal::model::grid::grid_handler::GridHandler;
use warp_terminal::model::grid::row::Row;
use warpui::{AppContext, ViewHandle};

use crate::terminal::model::terminal_model::TerminalModel;
use crate::terminal::view::TerminalView;

const HISTORY_BLOCKS: usize = 30;
const CLEAR_SCREEN: &str = "\u{1b}[H\u{1b}[2J\u{1b}[3J";

pub(crate) struct AttachSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub mode: TerminalMode,
    pub payload: String,
}

impl AttachSnapshot {
    pub fn encoded(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.payload.as_bytes())
    }
}

pub(crate) fn build_attach_snapshot(
    handle: &ViewHandle<TerminalView>,
    app: &AppContext,
) -> AttachSnapshot {
    let view = handle.as_ref(app);
    let model = view.model.lock();
    let size = model.block_list().size();
    let (cols, rows) = (size.columns, size.rows);
    let alt_screen_active = model.is_alt_screen_active();
    let mode = if alt_screen_active {
        TerminalMode::AltScreen
    } else if view.is_input_box_visible(&model, app) {
        TerminalMode::Prompt
    } else {
        TerminalMode::Running
    };

    let mut payload = String::new();
    payload.push_str(CLEAR_SCREEN);
    if !alt_screen_active {
        payload.push_str(&history_text(&model));
    }
    payload.push_str(&live_screen(&model, alt_screen_active, cols, rows));
    payload.push_str(&modes_to_ansi(current_modes(&model)));
    payload.push_str(&cursor_ansi(&model, alt_screen_active, rows));

    AttachSnapshot {
        cols,
        rows,
        mode,
        payload,
    }
}

fn history_text(model: &TerminalModel) -> String {
    let blocks = model.block_list().blocks();
    let active_index = model.block_list().active_block_index();
    let mut chunks: Vec<String> = Vec::new();
    let mut total = 0usize;
    for block in blocks
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != active_index.0)
        .map(|(_, block)| block)
        .rev()
        .take(HISTORY_BLOCKS)
    {
        let rendered = normalize_newlines(&block.command_and_output_to_string());
        if rendered.trim().is_empty() {
            continue;
        }
        total += rendered.len();
        if total > HISTORY_SNAPSHOT_MAX_BYTES as usize {
            break;
        }
        chunks.push(rendered);
    }
    chunks.reverse();
    if chunks.is_empty() {
        return String::new();
    }
    let mut out = chunks.join("\r\n");
    out.push_str("\r\n");
    out
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn live_screen(model: &TerminalModel, alt_screen_active: bool, cols: usize, rows: usize) -> String {
    if alt_screen_active {
        return visible_rows_to_ansi(model.alt_screen().grid_handler(), cols, rows);
    }
    let block = model.block_list().active_block();
    let rendered = normalize_newlines(&block.command_and_output_to_string());
    if rendered.trim().is_empty() {
        return String::new();
    }
    rendered
}

fn visible_rows_to_ansi(grid: &GridHandler, cols: usize, rows: usize) -> String {
    let total = grid.total_rows();
    let first = total.saturating_sub(rows);
    let owned: Vec<std::borrow::Cow<'_, Row>> =
        (first..total).filter_map(|index| grid.row(index)).collect();
    let borrowed: Vec<&Row> = owned.iter().map(|row| row.as_ref()).collect();
    grid_to_ansi(&borrowed, cols)
}

fn current_modes(model: &TerminalModel) -> TermMode {
    let tracked = [
        TermMode::SHOW_CURSOR,
        TermMode::APP_CURSOR,
        TermMode::APP_KEYPAD,
        TermMode::BRACKETED_PASTE,
        TermMode::LINE_WRAP,
        TermMode::ORIGIN,
        TermMode::INSERT,
        TermMode::MOUSE_REPORT_CLICK,
        TermMode::MOUSE_DRAG,
        TermMode::MOUSE_MOTION,
        TermMode::SGR_MOUSE,
        TermMode::FOCUS_IN_OUT,
    ];
    let mut mode = TermMode::empty();
    for flag in tracked {
        if model.is_term_mode_set(flag) {
            mode.insert(flag);
        }
    }
    mode
}

fn cursor_ansi(model: &TerminalModel, alt_screen_active: bool, rows: usize) -> String {
    if !alt_screen_active {
        return String::new();
    }
    let grid = model.alt_screen().grid_handler();
    let point = grid.cursor_render_point();
    let first_visible = grid.total_rows().saturating_sub(rows);
    let row = point.row.saturating_sub(first_visible);
    cursor_to_ansi(row.min(rows.saturating_sub(1)), point.col)
}
