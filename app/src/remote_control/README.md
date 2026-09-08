# Remote Control

The embedded browser client lives in `app/assets/web/remote_control`. Authenticated WebSocket
commands are routed through `commands.rs` to the desktop UI thread. `projection.rs` publishes
workspace state containing terminal panes; desktop Settings and other nonterminal tabs are omitted.
The browser Settings page controls the remote client.

## Native terminal mirror

`terminal.mirror` selects and reveals the requested desktop pane. `terminal.frame` captures the
rendered GPU surface, crops it to that pane at the window's backing scale, and returns a PNG with its
dimensions and fingerprint. Supplying `previous_frame` avoids encoding and transferring unchanged
pixels. Readback and encoding complete asynchronously, with one pending capture per window and a
three-second timeout. The selected pane and its bounds are checked again before returning the frame.

The browser requests one frame at a time, pauses in the background, and displays a recovery action
when the desktop cannot draw the pane. This mirrors the visible desktop session: changing terminals
remotely changes the desktop selection, and minimizing or occluding its window can suspend capture.
It does not create an independent offscreen desktop. A GPU surface must support frame readback.

`terminal.interact` maps normalized pointer coordinates, scrolling, text, and keys to native window
events. Keyboard input requires focus within the requested terminal. `terminal.selection` reads the
native selection for the browser's copy sheet. Mobile clients can scroll vertically, pan horizontally,
and adjust terminal zoom. Existing PTY attachment commands remain available to older protocol clients.

## Verification

Focused protocol, server, projection, and capture tests:

```sh
cargo nextest run -p remote_control -p warp -E 'package(remote_control) | test(remote_control::)'
```

Browser regression checks use Playwright and an installed Chrome, with a fixture WebSocket backend:

```sh
NODE_PATH=/path/to/node_modules node script/test_remote_control_web.mjs
```

Set `REMOTE_CONTROL_TEST_ARTIFACTS` to export desktop and mobile screenshots. These checks cover
launch forms, sheet navigation, failure recovery, mirror input, selection, and mobile layout.

The real-display integration test uses the production command handlers and a real shell in an
isolated test home. It checks Settings filtering, GPU capture, native command input, and terminal
creation. It requires an active graphical desktop and is ignored by the ordinary headless suite:

```sh
WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS=1 \
  cargo run -p integration --bin integration -- test_remote_control_mirror
WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS=1 \
  cargo nextest run -p integration --test integration \
  -E 'test(test_remote_control_mirror)' --run-ignored all
```

Set `WARP_REMOTE_MIRROR_ARTIFACTS` to save the terminal PNG from the native integration test.
