use remote_control::limits::MIRROR_QUEUE_FRAMES;
use remote_control::protocol::{
    CommandName, MIRROR_FRAME_KEY, MIRROR_FRAME_PATCH, MIRROR_FRAME_VERSION,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use warpui::integration::{AssertionOutcome, TestStep};
use warpui::{SingletonEntity as _, TypedActionView as _};

use super::view_getters::{single_terminal_view_for_tab, workspace_view};
use crate::remote_control::bridge::{ClientRegistration, CommandOutcome, RemoteControlBridge};
use crate::remote_control::mirror_encoder::MirrorPayload;
use crate::remote_control::{commands, projection};
use crate::workspace::WorkspaceAction;

const CLIENT: &str = "remote_control_client";
const TERMINAL: &str = "remote_control_terminal";
const MIRROR_PAYLOADS: &str = "remote_control_mirror_payloads";
const ENCODER_RUNTIME: &str = "remote_control_encoder_runtime";

pub fn connect_remote_control_client() -> TestStep {
    TestStep::new("Connect a remote client to the desktop terminal").with_action(
        |app, window_id, data| {
            let terminal = single_terminal_view_for_tab(app, window_id, 0);
            data.insert(TERMINAL, terminal.id().to_string());
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("encoder runtime");
            let handle = runtime.handle().clone();
            let (payload_tx, payload_rx) =
                mpsc::channel::<MirrorPayload>(MIRROR_QUEUE_FRAMES as usize);
            let client = RemoteControlBridge::handle(app).update(app, |bridge, ctx| {
                bridge.install_encoder_host(handle, ctx);
                let client = bridge.connect(ctx);
                bridge.set_mirror_sender(client.client_id, payload_tx);
                client
            });
            data.insert(CLIENT, client);
            data.insert(MIRROR_PAYLOADS, payload_rx);
            data.insert(ENCODER_RUNTIME, runtime);
        },
    )
}

pub fn remote_terminal_command(name: CommandName, interaction: Value) -> TestStep {
    TestStep::new(&format!("Remote command: {}", name.as_str())).with_action(move |app, _, data| {
        let id = data.get::<_, String>(TERMINAL).unwrap().clone();
        let client = data.get::<_, ClientRegistration>(CLIENT).unwrap().client_id;
        let mut params = interaction.clone();
        params["terminal_id"] = json!(id);
        RemoteControlBridge::handle(app).update(app, |bridge, ctx| {
            match commands::execute(bridge, client, "command".to_owned(), name, params, ctx) {
                CommandOutcome::Immediate(result) => {
                    result.expect("remote command succeeds");
                }
                CommandOutcome::Deferred => panic!("expected an immediate remote command"),
            }
        });
    })
}

struct MirrorRect {
    width: u32,
    height: u32,
    png: Vec<u8>,
}

fn parse_mirror_payload(payload: &[u8]) -> Option<(u8, u16, u16, Vec<MirrorRect>)> {
    let u16_at = |offset: usize| u16::from_le_bytes([payload[offset], payload[offset + 1]]);
    let u32_at = |offset: usize| {
        u32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ])
    };
    if payload.len() < 12 || payload[0] != MIRROR_FRAME_VERSION {
        return None;
    }
    let kind = payload[1];
    if kind != MIRROR_FRAME_KEY && kind != MIRROR_FRAME_PATCH {
        return None;
    }
    let (width, height, count) = (u16_at(6), u16_at(8), u16_at(10));
    let mut offset = 12;
    let mut rects = Vec::new();
    for _ in 0..count {
        let (rect_width, rect_height) = (u16_at(offset + 4), u16_at(offset + 6));
        let length = u32_at(offset + 8) as usize;
        rects.push(MirrorRect {
            width: u32::from(rect_width),
            height: u32::from(rect_height),
            png: payload[offset + 12..offset + 12 + length].to_vec(),
        });
        offset += 12 + length;
    }
    Some((kind, width, height, rects))
}

pub fn await_remote_terminal_frame() -> TestStep {
    TestStep::new("Receive native terminal pixels through remote control")
        .add_named_assertion_with_data_from_prior_step(
            "a mirror frame contains a nonempty PNG of the terminal",
            |_, _, data| {
                let payloads = data
                    .get_mut::<_, mpsc::Receiver<MirrorPayload>>(MIRROR_PAYLOADS)
                    .unwrap();
                while let Ok((_, payload)) = payloads.try_recv() {
                    let Some((kind, width, height, rects)) = parse_mirror_payload(&payload) else {
                        continue;
                    };
                    assert!(
                        width > 100 && height > 100,
                        "mirror frame is {width}x{height}"
                    );
                    assert!(!rects.is_empty());
                    let rect = &rects[0];
                    let image = image::load_from_memory(&rect.png).unwrap();
                    assert_eq!((image.width(), image.height()), (rect.width, rect.height));
                    if kind == MIRROR_FRAME_KEY {
                        let pixels = image.into_rgb8();
                        let first = pixels.get_pixel(0, 0);
                        assert!(
                            pixels.pixels().any(|pixel| pixel != first),
                            "keyframe must contain rendered content"
                        );
                        if let Ok(directory) = std::env::var("WARP_REMOTE_MIRROR_ARTIFACTS") {
                            std::fs::create_dir_all(&directory).unwrap();
                            std::fs::write(
                                std::path::Path::new(&directory).join("terminal.png"),
                                &rect.png,
                            )
                            .unwrap();
                        }
                    }
                    return AssertionOutcome::Success;
                }
                AssertionOutcome::failure("Waiting for the mirror frame".to_owned())
            },
        )
}

pub fn settings_are_not_remote_tabs() -> TestStep {
    TestStep::new("Desktop Settings is excluded from remote tabs")
        .with_action(|app, window_id, _| {
            workspace_view(app, window_id).update(app, |workspace, ctx| {
                workspace.handle_action(&WorkspaceAction::ShowSettings, ctx);
            });
        })
        .add_named_assertion(
            "remote workspace contains only its terminal",
            |app, window_id| {
                let workspace = workspace_view(app, window_id);
                workspace.read(app, |workspace, _| assert_eq!(workspace.tabs.len(), 2));
                app.read(|ctx| {
                    let snapshot = projection::build_snapshot("test", 1, None, ctx);
                    let tabs: Vec<_> = snapshot
                        .windows
                        .iter()
                        .flat_map(|window| &window.screens)
                        .flat_map(|screen| &screen.sections)
                        .flat_map(|section| &section.tabs)
                        .collect();
                    assert_eq!(tabs.len(), 1);
                    assert_eq!(tabs[0].kind, remote_control::protocol::TabKind::Terminal);
                });
                AssertionOutcome::Success
            },
        )
}

pub fn create_remote_terminal() -> TestStep {
    TestStep::new("Create a terminal from the remote client")
        .with_action(|app, window_id, data| {
            let screen = workspace_view(app, window_id).id().to_string();
            let client = data.get::<_, ClientRegistration>(CLIENT).unwrap().client_id;
            RemoteControlBridge::handle(app).update(app, |bridge, ctx| {
                match commands::execute(
                    bridge,
                    client,
                    "create".to_owned(),
                    CommandName::TerminalCreate,
                    json!({"screen_id": screen}),
                    ctx,
                ) {
                    CommandOutcome::Immediate(result) => {
                        let result = result.unwrap();
                        assert!(result["terminal_id"].is_string());
                    }
                    CommandOutcome::Deferred => panic!("terminal creation should be immediate"),
                }
            });
        })
        .add_named_assertion("desktop has a second terminal", |app, window_id| {
            workspace_view(app, window_id)
                .read(app, |workspace, _| assert_eq!(workspace.tabs.len(), 3));
            AssertionOutcome::Success
        })
}
