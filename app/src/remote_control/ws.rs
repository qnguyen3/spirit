use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use remote_control::limits::{
    CLIENT_IDLE_TIMEOUT_SECONDS, MAX_CLIENTS, MAX_INPUT_FRAME_BYTES, MAX_JSON_FRAME_BYTES,
};
use remote_control::protocol::{
    ClientMessage, CommandError, CommandName, ErrorCode, LimitEntry, ServerEvent, ServerMessage,
};
use remote_control::{PROTOCOL_VERSION, limits};
use tokio::sync::{broadcast, mpsc};

use super::bridge::{ClientId, CommandOutcome};
use super::http::{AppState, Authed, SameSiteOrigin, api_error};
use super::terminal_streams::{StreamFrame, output_queue_capacity};

pub(crate) async fn upgrade(
    State(state): State<Arc<AppState>>,
    _: SameSiteOrigin,
    authed: Authed,
    upgrade: WebSocketUpgrade,
) -> Response {
    let current = state.client_count.load(Ordering::Relaxed);
    if current as u64 >= MAX_CLIENTS {
        return api_error(
            ErrorCode::RateLimited,
            "too many devices are already connected",
        );
    }
    upgrade.on_upgrade(move |socket| run(socket, state, authed))
}

async fn run(socket: WebSocket, state: Arc<AppState>, authed: Authed) {
    state.client_count.fetch_add(1, Ordering::Relaxed);
    state.publish_client_count().await;

    let device_id = authed.device_id.clone();
    let registration = state
        .bridge_spawner
        .spawn(|bridge, ctx| bridge.connect(ctx))
        .await;
    let Ok(registration) = registration else {
        state.client_count.fetch_sub(1, Ordering::Relaxed);
        return;
    };
    let client_id = registration.client_id;

    let (stream_tx, stream_rx) = mpsc::channel(output_queue_capacity());
    let _ = state
        .bridge_spawner
        .spawn(move |bridge, _| {
            bridge.set_stream_sender(client_id, stream_tx);
            bridge.set_client_device(client_id, device_id);
        })
        .await;

    let hello = ServerMessage::Hello {
        instance_id: registration.instance_id.clone(),
        protocol: PROTOCOL_VERSION,
        app_version: state.app_version.clone(),
        client_id: client_id.as_string(),
        capabilities: CommandName::all()
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect(),
        limits: limits::all()
            .iter()
            .map(|(name, value)| LimitEntry {
                name: (*name).to_owned(),
                value: *value,
            })
            .collect(),
    };

    let broadcast_rx = state.broadcast.subscribe();
    pump(
        socket,
        state.clone(),
        client_id,
        hello,
        registration.latest,
        registration.receiver,
        stream_rx,
        broadcast_rx,
    )
    .await;

    let _ = state
        .bridge_spawner
        .spawn(move |bridge, _| bridge.disconnect(client_id))
        .await;
    state.client_count.fetch_sub(1, Ordering::Relaxed);
    state.publish_client_count().await;
}

#[allow(clippy::too_many_arguments)]
async fn pump(
    mut socket: WebSocket,
    state: Arc<AppState>,
    client_id: ClientId,
    hello: ServerMessage,
    initial_state: Option<Arc<ServerMessage>>,
    mut replies: mpsc::Receiver<Arc<ServerMessage>>,
    mut streams: mpsc::Receiver<(u32, StreamFrame)>,
    mut broadcast_rx: broadcast::Receiver<Arc<ServerMessage>>,
) {
    if send_json(&mut socket, &hello).await.is_err() {
        return;
    }
    if let Some(initial) = initial_state
        && send_json(&mut socket, &initial).await.is_err()
    {
        return;
    }

    let idle = Duration::from_secs(CLIENT_IDLE_TIMEOUT_SECONDS);
    loop {
        tokio::select! {
            incoming = tokio::time::timeout(idle, socket.recv()) => {
                let Ok(incoming) = incoming else {
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                };
                match incoming {
                    Some(Ok(message)) => {
                        if handle_incoming(message, &mut socket, &state, client_id)
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Some(Err(_)) | None => return,
                }
            }
            reply = replies.recv() => {
                let Some(reply) = reply else { return };
                if send_json(&mut socket, &reply).await.is_err() {
                    return;
                }
            }
            frame = streams.recv() => {
                let Some((attach_id, frame)) = frame else { continue };
                if send_stream_frame(&mut socket, &state, client_id, attach_id, frame)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            pushed = broadcast_rx.recv() => {
                match pushed {
                    Ok(message) => {
                        if send_json(&mut socket, &message).await.is_err() {
                            return;
                        }
                        if matches!(
                            message.as_ref(),
                            ServerMessage::Event { event: ServerEvent::ServerShuttingDown }
                        ) {
                            let _ = socket.send(Message::Close(None)).await;
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if resend_latest(&mut socket, &state).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

async fn handle_incoming(
    message: Message,
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    client_id: ClientId,
) -> Result<(), ()> {
    match message {
        Message::Text(text) => {
            if text.len() as u64 > MAX_JSON_FRAME_BYTES {
                let _ = socket.send(Message::Close(None)).await;
                return Err(());
            }
            handle_text(text.as_str(), socket, state, client_id).await
        }
        Message::Binary(bytes) => {
            if bytes.len() as u64 > MAX_INPUT_FRAME_BYTES + 4 {
                let _ = socket.send(Message::Close(None)).await;
                return Err(());
            }
            handle_binary(&bytes, state, client_id).await;
            Ok(())
        }
        Message::Ping(payload) => socket.send(Message::Pong(payload)).await.map_err(|_| ()),
        Message::Pong(_) => Ok(()),
        Message::Close(_) => Err(()),
    }
}

async fn handle_text(
    text: &str,
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    client_id: ClientId,
) -> Result<(), ()> {
    let parsed: Result<ClientMessage, _> = serde_json::from_str(text);
    let Ok(parsed) = parsed else {
        let id = extract_command_id(text);
        return match id {
            Some(id) => {
                let error = CommandError::invalid_request("that command could not be parsed");
                send_json(socket, &ServerMessage::error_result(id, error)).await
            }
            None => {
                let _ = socket.send(Message::Close(None)).await;
                Err(())
            }
        };
    };

    match parsed {
        ClientMessage::Ping { ts } => send_json(socket, &ServerMessage::Pong { ts }).await,
        ClientMessage::Command { id, name, params } => {
            let command_id = id.clone();
            let outcome = state
                .bridge_spawner
                .spawn(move |bridge, ctx| bridge.execute(client_id, command_id, name, params, ctx))
                .await;
            match outcome {
                Ok(CommandOutcome::Immediate(Ok(data))) => {
                    send_json(socket, &ServerMessage::ok_result(id, data)).await
                }
                Ok(CommandOutcome::Immediate(Err(error))) => {
                    send_json(socket, &ServerMessage::error_result(id, error)).await
                }
                Ok(CommandOutcome::Deferred) => Ok(()),
                Err(_) => {
                    let error =
                        CommandError::new(ErrorCode::BridgeUnavailable, "Spirit is shutting down");
                    send_json(socket, &ServerMessage::error_result(id, error)).await
                }
            }
        }
    }
}

fn extract_command_id(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value.get("id")?.as_str().map(str::to_owned)
}

async fn handle_binary(frame: &[u8], state: &Arc<AppState>, client_id: ClientId) {
    if frame.len() < 4 {
        return;
    }
    let attach_id = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
    let bytes = frame[4..].to_vec();
    if bytes.is_empty() {
        return;
    }
    let _ = state
        .bridge_spawner
        .spawn(move |bridge, ctx| bridge.write_attachment_input(client_id, attach_id, bytes, ctx))
        .await;
}

async fn send_stream_frame(
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    client_id: ClientId,
    attach_id: u32,
    frame: StreamFrame,
) -> Result<(), ()> {
    match frame {
        StreamFrame::Bytes(bytes) => {
            let mut payload = Vec::with_capacity(4 + bytes.len());
            payload.extend_from_slice(&attach_id.to_le_bytes());
            payload.extend_from_slice(&bytes);
            socket
                .send(Message::Binary(payload.into()))
                .await
                .map_err(|_| ())
        }
        StreamFrame::Overflowed => {
            let resync = state
                .bridge_spawner
                .spawn(move |bridge, ctx| bridge.build_resync(client_id, attach_id, ctx))
                .await;
            match resync {
                Ok(Some(event)) => send_json(socket, &ServerMessage::Event { event }).await,
                Ok(None) | Err(_) => Ok(()),
            }
        }
        StreamFrame::Closed => {
            send_json(
                socket,
                &ServerMessage::Event {
                    event: ServerEvent::TerminalClosed { attach_id },
                },
            )
            .await
        }
    }
}

async fn resend_latest(socket: &mut WebSocket, state: &Arc<AppState>) -> Result<(), ()> {
    let latest = state
        .bridge_spawner
        .spawn(|bridge, ctx| bridge.snapshot_now(ctx))
        .await;
    match latest {
        Ok(Ok(snapshot)) => {
            let message = ServerMessage::State {
                version: snapshot.version,
                snapshot: Box::new(snapshot),
            };
            send_json(socket, &message).await
        }
        Ok(Err(_)) | Err(_) => Ok(()),
    }
}

async fn send_json(socket: &mut WebSocket, message: &ServerMessage) -> Result<(), ()> {
    let Ok(encoded) = serde_json::to_string(message) else {
        return Ok(());
    };
    socket
        .send(Message::Text(Utf8Bytes::from(encoded)))
        .await
        .map_err(|_| ())
}
