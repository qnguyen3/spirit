use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use instant::Instant;
use parking_lot::Mutex;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::vec2f;
use remote_control::limits::MIRROR_CAPTURE_TIMEOUT_MS;
use remote_control::protocol::{CommandError, ErrorCode};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use warpui::r#async::{SpawnedFutureHandle, Timer};
use warpui::event::{Event, KeyEventDetails, ModifiersState};
use warpui::keymap::Keystroke;
use warpui::platform::{CapturedFrame, FrameObserver};
use warpui::zoom::Scale as _;
use warpui::{AppContext, ModelContext, SingletonEntity as _, WindowId};

use super::bridge::{ClientId, CommandOutcome, RemoteControlBridge};
use super::commands::activate_screen;
use super::mirror_encoder::{CropRect, MirrorFrame, MirrorState, run_encoder, state_payload};
use super::resolve::{self, TerminalTarget};
use crate::workspace::WorkspaceRegistry;
use crate::workspace::util::PaneViewLocator;

const RETRY_INTERVAL: Duration = Duration::from_millis(250);
const UNAVAILABLE_GRACE: Duration = Duration::from_secs(1);
const NOT_DRAWING_RETRY: Duration = Duration::from_secs(1);
const MIRROR_ID_MASK: u32 = 0x7FFF_FFFF;

#[derive(Deserialize)]
struct TerminalRef {
    terminal_id: String,
}

#[derive(Deserialize)]
struct MirrorRef {
    mirror_id: u32,
}

struct MirrorSession {
    mirror_id: u32,
    terminal_id: String,
    window_id: WindowId,
    frames: watch::Sender<Option<MirrorFrame>>,
    unavailable_since: Option<Instant>,
    reported_unavailable: bool,
}

type Target = (watch::Sender<Option<MirrorFrame>>, CropRect);

#[derive(Default)]
struct WindowCapture {
    armed: bool,
    observing: bool,
    next_seq: Arc<AtomicU64>,
    targets: Arc<Mutex<Vec<Target>>>,
    arm_timer: Option<SpawnedFutureHandle>,
    timeout: Option<SpawnedFutureHandle>,
}

impl WindowCapture {
    fn clear_timers(&mut self) {
        replace_timer(&mut self.arm_timer, None);
        replace_timer(&mut self.timeout, None);
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
        self.clear_timers();
    }
}

fn replace_timer(slot: &mut Option<SpawnedFutureHandle>, next: Option<SpawnedFutureHandle>) {
    if let Some(previous) = slot.take() {
        previous.abort();
    }
    *slot = next;
}

#[derive(Default)]
pub(crate) struct MirrorHub {
    sessions: HashMap<ClientId, MirrorSession>,
    windows: HashMap<WindowId, WindowCapture>,
    next_mirror_id: u32,
}

impl MirrorHub {
    fn clients_on(&self, window_id: WindowId) -> Vec<ClientId> {
        self.sessions
            .iter()
            .filter(|(_, session)| session.window_id == window_id)
            .map(|(client_id, _)| *client_id)
            .collect()
    }
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

pub(super) fn open(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
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
        let mirror_id = start(
            bridge,
            client_id,
            request.terminal_id,
            target.window_id,
            ctx,
        )?;
        Ok(json!({ "mirror_id": mirror_id }))
    })();
    CommandOutcome::Immediate(result)
}

pub(super) fn stop_command(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    raw: Value,
) -> CommandOutcome {
    let result = (|| {
        let request: MirrorRef = serde_json::from_value(raw)
            .map_err(|_| CommandError::invalid_request("mirror_id is required"))?;
        if stop(bridge, client_id, request.mirror_id) {
            Ok(json!({}))
        } else {
            Err(CommandError::not_found("that mirror"))
        }
    })();
    CommandOutcome::Immediate(result)
}

fn start(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    terminal_id: String,
    window_id: WindowId,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Result<u32, CommandError> {
    let closing = || CommandError::new(ErrorCode::BridgeUnavailable, "this connection is closing");
    let payloads = bridge.mirror_sender(client_id).ok_or_else(closing)?;
    let (runtime, spawner) = bridge
        .encoder_host()
        .map(|host| (host.runtime.clone(), host.spawner.clone()))
        .ok_or_else(closing)?;
    stop_client(bridge, client_id);
    let hub = &mut bridge.mirrors;
    hub.next_mirror_id = (hub.next_mirror_id.wrapping_add(1) & MIRROR_ID_MASK).max(1);
    let mirror_id = hub.next_mirror_id;
    let (frames, receiver) = watch::channel(None);
    runtime.spawn(run_encoder(
        receiver, payloads, spawner, client_id, mirror_id,
    ));
    hub.sessions.insert(
        client_id,
        MirrorSession {
            mirror_id,
            terminal_id,
            window_id,
            frames,
            unavailable_since: None,
            reported_unavailable: false,
        },
    );
    arm_window(bridge, window_id, ctx);
    Ok(mirror_id)
}

fn stop(bridge: &mut RemoteControlBridge, client_id: ClientId, mirror_id: u32) -> bool {
    let matches = bridge
        .mirrors
        .sessions
        .get(&client_id)
        .is_some_and(|session| session.mirror_id == mirror_id);
    if matches {
        stop_client(bridge, client_id);
    }
    matches
}

pub(crate) fn stop_client(bridge: &mut RemoteControlBridge, client_id: ClientId) {
    let Some(session) = bridge.mirrors.sessions.remove(&client_id) else {
        return;
    };
    if bridge.mirrors.clients_on(session.window_id).is_empty() {
        bridge.mirrors.windows.remove(&session.window_id);
    }
}

pub(crate) fn stop_all(bridge: &mut RemoteControlBridge) {
    bridge.mirrors.sessions.clear();
    bridge.mirrors.windows.clear();
}

fn crop_for(bounds: RectF, scale: f32) -> CropRect {
    CropRect {
        x: (bounds.origin().x() * scale).round().max(0.0) as u32,
        y: (bounds.origin().y() * scale).round().max(0.0) as u32,
        width: (bounds.width() * scale).round().max(0.0) as u32,
        height: (bounds.height() * scale).round().max(0.0) as u32,
    }
}

fn send_state(
    bridge: &RemoteControlBridge,
    client_id: ClientId,
    mirror_id: u32,
    state: MirrorState,
    message: &str,
) {
    if let Some(sender) = bridge.mirror_sender(client_id) {
        let _ = sender.try_send((mirror_id, state_payload(state, message)));
    }
}

fn note_failure(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    error: CommandError,
    now: Instant,
) {
    let Some(session) = bridge.mirrors.sessions.get_mut(&client_id) else {
        return;
    };
    let mirror_id = session.mirror_id;
    match error.code {
        ErrorCode::NotFound => {
            send_state(
                bridge,
                client_id,
                mirror_id,
                MirrorState::Unavailable,
                "This terminal was closed.",
            );
            stop_client(bridge, client_id);
        }
        ErrorCode::Unauthorized
        | ErrorCode::ForbiddenOrigin
        | ErrorCode::BadHost
        | ErrorCode::FeatureDisabled
        | ErrorCode::InvalidRequest
        | ErrorCode::UnknownCommand
        | ErrorCode::NotAtPrompt
        | ErrorCode::NoAgentSession
        | ErrorCode::TerminalReadOnly
        | ErrorCode::Conflict
        | ErrorCode::NotGitProject
        | ErrorCode::GitFailed
        | ErrorCode::PayloadTooLarge
        | ErrorCode::TooManyAttachments
        | ErrorCode::RateLimited
        | ErrorCode::BridgeUnavailable
        | ErrorCode::Unsupported => {
            let since = *session.unavailable_since.get_or_insert(now);
            if !session.reported_unavailable && now.duration_since(since) >= UNAVAILABLE_GRACE {
                session.reported_unavailable = true;
                send_state(
                    bridge,
                    client_id,
                    mirror_id,
                    MirrorState::Unavailable,
                    &error.message,
                );
            }
        }
    }
}

fn note_success(bridge: &mut RemoteControlBridge, client_id: ClientId) {
    let Some(session) = bridge.mirrors.sessions.get_mut(&client_id) else {
        return;
    };
    session.unavailable_since = None;
    if session.reported_unavailable {
        session.reported_unavailable = false;
        let mirror_id = session.mirror_id;
        send_state(bridge, client_id, mirror_id, MirrorState::Live, "");
    }
}

fn compute_targets(
    bridge: &mut RemoteControlBridge,
    window_id: WindowId,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Vec<Target> {
    let now = Instant::now();
    let zoom = ctx.zoom_factor();
    let scale = ctx
        .windows()
        .platform_window(window_id)
        .map(|window| window.as_ctx().backing_scale_factor().scale_up(zoom));
    let mut targets = Vec::new();
    let mut successes = Vec::new();
    let mut failures = Vec::new();
    for client_id in bridge.mirrors.clients_on(window_id) {
        let Some(session) = bridge.mirrors.sessions.get(&client_id) else {
            continue;
        };
        let Some(scale) = scale else {
            failures.push((client_id, CommandError::not_found("that window")));
            continue;
        };
        match visible_terminal(&session.terminal_id, ctx) {
            Ok((_, bounds)) => {
                targets.push((session.frames.clone(), crop_for(bounds, scale)));
                successes.push(client_id);
            }
            Err(error) => failures.push((client_id, error)),
        }
    }
    for (client_id, error) in failures {
        note_failure(bridge, client_id, error, now);
    }
    for client_id in successes {
        note_success(bridge, client_id);
    }
    targets
}

fn deliver(targets: &Weak<Mutex<Vec<Target>>>, next_seq: &AtomicU64, frame: CapturedFrame) -> bool {
    let Some(targets) = targets.upgrade() else {
        return false;
    };
    let frame = Arc::new(frame);
    let seq = next_seq.fetch_add(1, Ordering::Relaxed) + 1;
    for (sender, crop) in targets.lock().iter() {
        sender.send_replace(Some(MirrorFrame {
            frame: frame.clone(),
            crop: *crop,
            seq,
        }));
    }
    true
}

fn frame_observer(targets: Weak<Mutex<Vec<Target>>>, next_seq: Arc<AtomicU64>) -> FrameObserver {
    Box::new(move |frame| deliver(&targets, &next_seq, frame))
}

fn schedule_retry(
    capture: &mut WindowCapture,
    window_id: WindowId,
    ctx: &mut ModelContext<RemoteControlBridge>,
) {
    let retry = ctx.spawn(
        async move { Timer::after(RETRY_INTERVAL).await },
        move |bridge, _, ctx| arm_window(bridge, window_id, ctx),
    );
    replace_timer(&mut capture.arm_timer, Some(retry));
}

fn arm_window(
    bridge: &mut RemoteControlBridge,
    window_id: WindowId,
    ctx: &mut ModelContext<RemoteControlBridge>,
) {
    if bridge.mirrors.clients_on(window_id).is_empty() {
        bridge.mirrors.windows.remove(&window_id);
        return;
    }
    let targets = compute_targets(bridge, window_id, ctx);
    if bridge.mirrors.clients_on(window_id).is_empty() {
        bridge.mirrors.windows.remove(&window_id);
        return;
    }
    let capture = bridge.mirrors.windows.entry(window_id).or_default();
    replace_timer(&mut capture.arm_timer, None);
    if targets.is_empty() {
        capture.armed = false;
        capture.targets.lock().clear();
        schedule_retry(capture, window_id, ctx);
        return;
    }
    *capture.targets.lock() = targets;
    capture.armed = true;
    let weak_targets = Arc::downgrade(&capture.targets);
    let next_seq = capture.next_seq.clone();
    let install_observer = !capture.observing;
    capture.observing = true;
    if let Some(window) = ctx.windows().platform_window(window_id) {
        if install_observer {
            window
                .as_ctx()
                .set_frame_observer(Some(frame_observer(weak_targets.clone(), next_seq.clone())));
        }
        window
            .as_ctx()
            .request_frame_capture(Box::new(move |frame| {
                deliver(&weak_targets, &next_seq, frame);
            }));
    }
    let timeout = ctx.spawn(
        async move { Timer::after(Duration::from_millis(MIRROR_CAPTURE_TIMEOUT_MS)).await },
        move |bridge, _, ctx| capture_timed_out(bridge, window_id, ctx),
    );
    if let Some(capture) = bridge.mirrors.windows.get_mut(&window_id) {
        replace_timer(&mut capture.timeout, Some(timeout));
    }
}

fn capture_timed_out(
    bridge: &mut RemoteControlBridge,
    window_id: WindowId,
    ctx: &mut ModelContext<RemoteControlBridge>,
) {
    if !bridge
        .mirrors
        .windows
        .get(&window_id)
        .is_some_and(|capture| capture.armed)
    {
        return;
    }
    for client_id in bridge.mirrors.clients_on(window_id) {
        let Some(session) = bridge.mirrors.sessions.get_mut(&client_id) else {
            continue;
        };
        session.reported_unavailable = true;
        session.unavailable_since.get_or_insert(Instant::now());
        let mirror_id = session.mirror_id;
        send_state(
            bridge,
            client_id,
            mirror_id,
            MirrorState::Unavailable,
            "The desktop is not drawing. Restore its window and retry.",
        );
    }
    let Some(capture) = bridge.mirrors.windows.get_mut(&window_id) else {
        return;
    };
    capture.armed = false;
    let retry = ctx.spawn(
        async move { Timer::after(NOT_DRAWING_RETRY).await },
        move |bridge, _, ctx| arm_window(bridge, window_id, ctx),
    );
    replace_timer(&mut capture.arm_timer, Some(retry));
}

pub(crate) fn frame_done(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    mirror_id: u32,
    ctx: &mut ModelContext<RemoteControlBridge>,
) {
    let Some(session) = bridge.mirrors.sessions.get(&client_id) else {
        return;
    };
    if session.mirror_id != mirror_id {
        return;
    }
    let window_id = session.window_id;
    let Some(capture) = bridge.mirrors.windows.get_mut(&window_id) else {
        return;
    };
    capture.armed = false;
    replace_timer(&mut capture.timeout, None);
    let targets = compute_targets(bridge, window_id, ctx);
    let Some(capture) = bridge.mirrors.windows.get_mut(&window_id) else {
        return;
    };
    if targets.is_empty() && capture.arm_timer.is_none() {
        schedule_retry(capture, window_id, ctx);
    }
    *capture.targets.lock() = targets;
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

fn typed_chars_for_key(key: &str) -> Option<&'static str> {
    match key {
        "enter" => Some("\r"),
        "tab" => Some("\t"),
        "escape" => Some("\x1b"),
        "backspace" => Some("\x7f"),
        _ => None,
    }
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
        let zoom = ctx.zoom_factor();
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
            Ok((bounds.origin()
                + vec2f(
                    x * (bounds.width() - 1.0).max(0.0),
                    y * (bounds.height() - 1.0).max(0.0),
                ))
            .scale_up(zoom))
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
                let chars = if chars.is_empty() {
                    typed_chars_for_key(&key).unwrap_or_default().to_owned()
                } else {
                    chars
                };
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
