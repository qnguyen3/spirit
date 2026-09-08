use remote_control::protocol::CommandName;
use serde_json::json;
use warp::integration_testing::remote_control::{
    capture_remote_terminal_frame, connect_remote_control_client, create_remote_terminal,
    remote_terminal_command, settings_are_not_remote_tabs,
};
use warp::integration_testing::terminal::{
    assert_command_executed_for_single_terminal_in_tab, wait_until_bootstrapped_single_pane_for_tab,
};
use warpui_core::integration::TestStep;

use crate::Builder;

pub fn test_remote_control_mirror() -> Builder {
    Builder::new()
        .with_real_display()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(connect_remote_control_client())
        .with_step(settings_are_not_remote_tabs())
        .with_step(remote_terminal_command(
            CommandName::TerminalMirror,
            json!({}),
        ))
        .with_step(capture_remote_terminal_frame())
        .with_step(remote_terminal_command(
            CommandName::TerminalInteract,
            json!({"kind": "text", "text": "echo remote-mirror-smoke"}),
        ))
        .with_step(remote_terminal_command(
            CommandName::TerminalInteract,
            json!({"kind": "key", "key": "enter"}),
        ))
        .with_step(
            TestStep::new("Native input runs the remote command")
                .add_assertion(assert_command_executed_for_single_terminal_in_tab(
                    0,
                    "echo remote-mirror-smoke".to_owned(),
                ))
                .with_take_screenshot("desktop.png"),
        )
        .with_step(capture_remote_terminal_frame())
        .with_step(create_remote_terminal())
        .with_step(wait_until_bootstrapped_single_pane_for_tab(1))
}
