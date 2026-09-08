# Remote Control

The embedded browser client lives in `app/assets/web/remote_control`. Authenticated WebSocket
commands are routed through `commands.rs` to the desktop UI thread. `projection.rs` publishes
workspace state containing terminal panes; desktop Settings and other nonterminal tabs are omitted.
The browser Settings page controls the remote client.

## Native terminal mirror

`terminal.mirror` selects and reveals the requested desktop pane and starts a push session; it
returns a `mirror_id`. From then on the desktop captures the rendered GPU surface, crops it to the
pane at the window's backing scale, diffs it against the previous capture, and pushes only the
changed rectangles as opaque RGB PNGs over the WebSocket. Binary frames carry the channel
`0x80000000 | mirror_id` in their first four little-endian bytes, followed by
`[u8 version=1][u8 kind][u32 seq][u16 width][u16 height][u16 rect_count]` and, per rectangle,
`[u16 x][u16 y][u16 w][u16 h][u32 png_len][png]`. Kind 1 is a keyframe covering the whole pane,
kind 2 a set of patches, and kind 3 a state frame `[u8 state][u16 len][utf8 message]` where state
1 means the pane is unavailable (hidden, moved, closed, or the desktop stopped drawing) and 0 means
it is live again. `terminal.mirror_stop` ends the session; disconnecting does too.

Captures never force a scene rebuild. While a pane is mirrored the window keeps a frame observer
that receives every frame the desktop draws on its own, which costs nothing while idle, so a
program's echo of a keystroke reaches the browser on the very frame that shows it. A forced
re-render of the cached scene is used only to produce the first keyframe of a session and to check
that the desktop is drawing. Frames are encoded off the main thread and the encoder always works on
the latest frame; a slow client only delays or skips frames and the next frame becomes a keyframe.
Several devices mirroring panes in one window share a single observer, and the pane crops are
refreshed after every encoded frame. A forced capture that produces no frame within
`mirror_capture_timeout_ms` reports the desktop as not drawing and retries every second.

The browser draws frames in order onto a canvas, pauses the session while in the background, and
displays a recovery action when the desktop cannot draw the pane. This mirrors the visible desktop
session: changing terminals remotely changes the desktop selection, and minimizing or occluding its
window can suspend capture. It does not create an independent offscreen desktop. A GPU surface must
support frame readback.

`terminal.interact` maps normalized pointer coordinates, scrolling, text, and keys to native window
events. Keyboard input requires focus within the requested terminal. Named keys without typed
characters (Enter, Tab, Escape, Backspace) fall back to the characters the native platform would
type. `terminal.selection` reads the native selection for the browser's copy sheet. Mobile clients
can scroll vertically, pan horizontally, and adjust terminal zoom. Existing PTY attachment commands
remain available to older protocol clients.

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
