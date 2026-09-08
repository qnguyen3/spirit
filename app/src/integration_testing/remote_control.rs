use base64::Engine as _;
use remote_control::protocol::{CommandName, ServerMessage};
use serde_json::{Value, json};
use warpui::integration::{AssertionOutcome, TestStep};
use warpui::{SingletonEntity as _, TypedActionView as _};

use super::view_getters::{single_terminal_view_for_tab, workspace_view};
use crate::remote_control::bridge::{ClientRegistration, CommandOutcome, RemoteControlBridge};
use crate::remote_control::{commands, projection};
use crate::workspace::WorkspaceAction;

const CLIENT: &str = "remote_control_client";
const TERMINAL: &str = "remote_control_terminal";

pub fn connect_remote_control_client() -> TestStep {
    TestStep::new("Connect a remote client to the desktop terminal").with_action(
        |app, window_id, data| {
            let terminal = single_terminal_view_for_tab(app, window_id, 0);
            data.insert(TERMINAL, terminal.id().to_string());
            let client =
                RemoteControlBridge::handle(app).update(app, |bridge, ctx| bridge.connect(ctx));
            data.insert(CLIENT, client);
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

pub fn capture_remote_terminal_frame() -> TestStep {
    TestStep::new("Capture native terminal pixels through remote control")
        .with_action(|app, _, data| {
            let id = data.get::<_, String>(TERMINAL).unwrap().clone();
            let client = data.get::<_, ClientRegistration>(CLIENT).unwrap().client_id;
            RemoteControlBridge::handle(app).update(app, |bridge, ctx| {
                match commands::execute(
                    bridge,
                    client,
                    "frame".to_owned(),
                    CommandName::TerminalFrame,
                    json!({"terminal_id": id}),
                    ctx,
                ) {
                    CommandOutcome::Deferred => {}
                    CommandOutcome::Immediate(result) => {
                        panic!("expected a captured frame: {result:?}")
                    }
                }
            });
        })
        .add_named_assertion_with_data_from_prior_step(
            "frame contains a nonempty PNG of the terminal",
            |_, _, data| {
                let client = data.get_mut::<_, ClientRegistration>(CLIENT).unwrap();
                while let Ok(message) = client.receiver.try_recv() {
                    if let ServerMessage::Result {
                        id,
                        ok,
                        data,
                        error,
                    } = message.as_ref()
                        && id == "frame"
                    {
                        assert!(ok, "frame capture failed: {error:?}");
                        let data = data.as_ref().unwrap();
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(data["image"].as_str().unwrap())
                            .unwrap();
                        let image = image::load_from_memory(&bytes).unwrap();
                        assert!(image.width() > 100 && image.height() > 100);
                        let pixels = image.into_rgba8();
                        let first = pixels.get_pixel(0, 0);
                        assert!(
                            pixels.pixels().any(|pixel| pixel != first),
                            "frame must contain rendered content"
                        );
                        if let Ok(directory) = std::env::var("WARP_REMOTE_MIRROR_ARTIFACTS") {
                            std::fs::create_dir_all(&directory).unwrap();
                            std::fs::write(
                                std::path::Path::new(&directory).join("terminal.png"),
                                bytes,
                            )
                            .unwrap();
                        }
                        return AssertionOutcome::Success;
                    }
                }
                AssertionOutcome::failure("Waiting for the GPU frame".to_owned())
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
