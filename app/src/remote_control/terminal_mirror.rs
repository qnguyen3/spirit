use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use base64::Engine as _;
use image::ImageEncoder as _;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::vec2f;
use remote_control::protocol::{CommandError, ErrorCode};
use serde::Deserialize;
use serde_json::{Value, json};
use warpui::r#async::FutureExt as _;
use warpui::event::{Event, KeyEventDetails, ModifiersState};
use warpui::keymap::Keystroke;
use warpui::platform::CapturedFrame;
use warpui::{AppContext, ModelContext, SingletonEntity as _};

use super::bridge::{ClientId, CommandOutcome, RemoteControlBridge};
use super::commands::activate_screen;
use super::resolve::{self, TerminalTarget};
use crate::workspace::WorkspaceRegistry;
use crate::workspace::util::PaneViewLocator;

#[derive(Deserialize)]
struct TerminalRef {
    terminal_id: String,
    #[serde(default)]
    previous_frame: Option<String>,
}

fn unavailable(message: &str) -> CommandError {
    CommandError::new(ErrorCode::Conflict, message)
}

fn visible_terminal(id: &str, ctx: &AppContext) -> Result<(TerminalTarget, RectF), CommandError> {
    let target = resolve::terminal(id, ctx)?;
    let workspace = target.workspace.as_ref(ctx);
    if WorkspaceRegistry::as_ref(ctx).active_workspace_view_id(target.window_id)
        != Some(target.workspace.id())
        || workspace
            .tabs
            .get(workspace.active_tab_index())
            .map(|tab| tab.pane_group.id())
            != Some(target.pane_group.id())
        || !target
            .pane_group
            .as_ref(ctx)
            .visible_pane_ids()
            .contains(&target.pane_id)
    {
        return Err(unavailable(
            "This terminal is no longer visible on the desktop.",
        ));
    }
    let bounds = ctx
        .element_position_by_id_at_last_frame(
            target.window_id,
            target.terminal.as_ref(ctx).terminal_position_id(),
        )
        .filter(|bounds| bounds.width() > 0.0 && bounds.height() > 0.0)
        .ok_or_else(|| unavailable("Waiting for the desktop to draw this terminal."))?;
    Ok((target, bounds))
}

pub(super) fn open(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let result = (|| {
        let request: TerminalRef = serde_json::from_value(raw)
            .map_err(|_| CommandError::invalid_request("terminal_id is required"))?;
        let target = resolve::terminal(&request.terminal_id, ctx)?;
        activate_screen(&target.workspace.id().to_string(), ctx)?;
        ctx.windows().show_window_and_focus_app(target.window_id);
        target.workspace.update(ctx, |workspace, ctx| {
            workspace.focus_pane(
                PaneViewLocator {
                    pane_group_id: target.pane_group.id(),
                    pane_id: target.pane_id,
                },
                ctx,
            );
        });
        Ok(json!({}))
    })();
    CommandOutcome::Immediate(result)
}

pub(super) fn selection(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let result = (|| {
        let request: TerminalRef = serde_json::from_value(raw)
            .map_err(|_| CommandError::invalid_request("terminal_id is required"))?;
        let target = resolve::terminal(&request.terminal_id, ctx)?;
        let view = target.terminal.as_ref(ctx);
        let text = view
            .selected_text_from_input(ctx)
            .or_else(|| view.selected_text(ctx))
            .unwrap_or_default();
        Ok(json!({ "text": text }))
    })();
    CommandOutcome::Immediate(result)
}

pub(super) fn capture(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: TerminalRef = match serde_json::from_value(raw) {
        Ok(request) => request,
        Err(_) => {
            return CommandOutcome::Immediate(Err(CommandError::invalid_request(
                "terminal_id is required",
            )));
        }
    };
    let (target, bounds) = match visible_terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let window_id = target.window_id;
    let Some(window) = ctx.windows().platform_window(window_id) else {
        return CommandOutcome::Immediate(Err(CommandError::not_found("that window")));
    };
    if !bridge.capturing_windows.insert(window_id) {
        return CommandOutcome::Immediate(Err(CommandError::new(
            ErrorCode::RateLimited,
            "Waiting for the next desktop frame.",
        )));
    }
    let scale = window.as_ctx().backing_scale_factor();
    let previous_frame = request.previous_frame;
    let (tx, rx) = futures::channel::oneshot::channel();
    window
        .as_ctx()
        .request_frame_capture(Box::new(move |frame| {
            let _ = tx.send(frame);
        }));
    window.as_ctx().request_redraw();
    ctx.spawn(
        async move {
            let frame = rx
                .with_timeout(Duration::from_secs(3))
                .await
                .map_err(|_| {
                    unavailable("The desktop is not drawing. Restore its window and retry.")
                })?
                .map_err(|_| unavailable("The desktop frame was interrupted."))?;
            encode_frame(frame, bounds, scale, previous_frame.as_deref())
        },
        move |bridge, result, ctx| {
            bridge.capturing_windows.remove(&window_id);
            // A tab switch or resize during GPU readback invalidates the crop.
            let result = visible_terminal(&request.terminal_id, ctx).and_then(|(_, current)| {
                if current != bounds {
                    Err(unavailable(
                        "The terminal moved. Waiting for the next frame.",
                    ))
                } else {
                    result
                }
            });
            bridge.resolve_deferred(client_id, command_id, result);
        },
    );
    CommandOutcome::Deferred
}

fn encode_frame(
    mut frame: CapturedFrame,
    bounds: RectF,
    scale: f32,
    previous_frame: Option<&str>,
) -> Result<Value, CommandError> {
    let x = (bounds.origin().x() * scale).round().max(0.0) as u32;
    let y = (bounds.origin().y() * scale).round().max(0.0) as u32;
    let width = (bounds.width() * scale).round().max(0.0) as u32;
    let height = (bounds.height() * scale).round().max(0.0) as u32;
    if width == 0
        || height == 0
        || x.saturating_add(width) > frame.width
        || y.saturating_add(height) > frame.height
        || frame.data.len() != frame.width as usize * frame.height as usize * 4
    {
        return Err(unavailable("The terminal frame changed size. Retrying."));
    }
    frame.ensure_rgba();
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for row in y..y + height {
        let offset = (row as usize * frame.width as usize + x as usize) * 4;
        pixels.extend_from_slice(&frame.data[offset..offset + width as usize * 4]);
    }
    let mut hash = DefaultHasher::new();
    (width, height, &pixels).hash(&mut hash);
    let fingerprint = format!("{:016x}", hash.finish());
    if previous_frame == Some(fingerprint.as_str()) {
        return Ok(
            json!({ "image": null, "fingerprint": fingerprint, "width": width, "height": height }),
        );
    }
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|_| unavailable("Could not encode the desktop frame."))?;
    Ok(json!({
        "image": base64::engine::general_purpose::STANDARD.encode(png),
        "width": width,
        "height": height,
        "fingerprint": fingerprint,
    }))
}

#[derive(Default, Deserialize)]
struct Modifiers {
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    cmd: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Interaction {
    Key {
        key: String,
        #[serde(default)]
        chars: String,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Text {
        text: String,
    },
    Pointer {
        phase: PointerPhase,
        x: f32,
        y: f32,
        #[serde(default)]
        modifiers: Modifiers,
    },
    Scroll {
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PointerPhase {
    Down,
    Up,
    Drag,
}

#[derive(Deserialize)]
struct Interact {
    terminal_id: String,
    #[serde(flatten)]
    interaction: Interaction,
}

pub(super) fn interact(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let result = (|| {
        let request: Interact = serde_json::from_value(raw)
            .map_err(|_| CommandError::invalid_request("invalid terminal interaction"))?;
        let (target, bounds) = visible_terminal(&request.terminal_id, ctx)?;
        if matches!(
            &request.interaction,
            Interaction::Key { .. } | Interaction::Text { .. }
        ) && target.terminal.as_ref(ctx).model.lock().is_read_only()
        {
            return Err(CommandError::new(
                ErrorCode::TerminalReadOnly,
                "That terminal has exited.",
            ));
        }
        let point = |x: f32, y: f32| {
            if !x.is_finite()
                || !y.is_finite()
                || !(0.0..=1.0).contains(&x)
                || !(0.0..=1.0).contains(&y)
            {
                return Err(CommandError::invalid_request(
                    "pointer is outside the terminal",
                ));
            }
            Ok(bounds.origin()
                + vec2f(
                    x * (bounds.width() - 1.0).max(0.0),
                    y * (bounds.height() - 1.0).max(0.0),
                ))
        };
        let event = match request.interaction {
            Interaction::Key {
                key,
                chars,
                modifiers,
            } => {
                if !Keystroke::is_valid_key(&key) || chars.len() > 128 {
                    return Err(CommandError::invalid_request("invalid key"));
                }
                if !target.terminal.is_self_or_child_focused(ctx) {
                    return Err(unavailable("Click the terminal to focus its input first."));
                }
                let keystroke = Keystroke {
                    key,
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    cmd: modifiers.cmd,
                    meta: false,
                };
                let handled = ctx.dispatch_window_input(
                    Event::KeyDown {
                        keystroke,
                        chars: chars.clone(),
                        details: KeyEventDetails::default(),
                        is_composing: false,
                    },
                    target.window_id,
                );
                if !handled && !chars.is_empty() && !modifiers.ctrl && !modifiers.cmd {
                    ctx.dispatch_window_input(Event::TypedCharacters { chars }, target.window_id);
                }
                return Ok(json!({}));
            }
            Interaction::Text { text } => {
                if text.len() > remote_control::limits::MAX_INPUT_FRAME_BYTES as usize {
                    return Err(CommandError::invalid_request("text is too large"));
                }
                if !target.terminal.is_self_or_child_focused(ctx) {
                    return Err(unavailable("Click the terminal to focus its input first."));
                }
                Event::TypedCharacters { chars: text }
            }
            Interaction::Pointer {
                phase,
                x,
                y,
                modifiers,
            } => {
                let position = point(x, y)?;
                let modifiers = ModifiersState {
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    cmd: modifiers.cmd,
                    ..ModifiersState::default()
                };
                match phase {
                    PointerPhase::Down => Event::LeftMouseDown {
                        position,
                        modifiers,
                        click_count: 1,
                        is_first_mouse: false,
                    },
                    PointerPhase::Up => Event::LeftMouseUp {
                        position,
                        modifiers,
                    },
                    PointerPhase::Drag => Event::LeftMouseDragged {
                        position,
                        modifiers,
                    },
                }
            }
            Interaction::Scroll {
                x,
                y,
                delta_x,
                delta_y,
            } => {
                if !delta_x.is_finite() || !delta_y.is_finite() {
                    return Err(CommandError::invalid_request("invalid scroll delta"));
                }
                Event::ScrollWheel {
                    position: point(x, y)?,
                    delta: vec2f(
                        -delta_x.clamp(-1000.0, 1000.0),
                        -delta_y.clamp(-1000.0, 1000.0),
                    ),
                    precise: true,
                    modifiers: ModifiersState::default(),
                }
            }
        };
        ctx.dispatch_window_input(event, target.window_id);
        Ok(json!({}))
    })();
    CommandOutcome::Immediate(result)
}

#[cfg(test)]
#[path = "terminal_mirror_tests.rs"]
mod tests;
