# Remote Control — Closeout

Closeout for the effort specced in `spirit_specs/0907-remote-control/`.
Baseline: commit `56530e0` on branch `feat-remote-control`.

## What landed

A toggle in Settings → Remote Control starts an HTTP + WebSocket server inside the
running app and serves a responsive web client that mirrors and drives the desktop
from a phone, tablet or laptop browser.

### New crates and modules

| Path | Contents |
|---|---|
| `crates/remote_control/` | Protocol/security kernel with no app dependencies: `protocol.rs` (wire types, `CommandName`, `ErrorCode`), `auth.rs` (`AccessToken`, `SessionId`, `PairedDevice`, constant-time comparison), `hosts.rs` (`AllowedHosts`, `RemoteEndpoint`), `limits.rs` |
| `crates/warp_terminal/src/model/ansi_export.rs` | `grid_to_ansi`, `modes_to_ansi`, `cursor_to_ansi` |
| `app/src/remote_control/` | Server (`mod.rs`), main-thread bridge (`bridge.rs`), projection, watchers, commands, resolver, HTTP router, cookie sessions, embedded assets, LAN discovery, WebSocket, terminal snapshot and PTY relay |
| `app/src/settings/remote_control.rs` | Public settings group (`enabled`, `port`, `allow_lan_access`) |
| `app/src/settings/remote_control_secrets.rs` | Secure access token plus hashed paired-device list |
| `app/src/settings_view/remote_control_page.rs` | Settings page |
| `app/assets/web/remote_control/` | The browser client (no build step, vendored xterm.js) |
| `script/vendor_remote_control_assets` | Downloads and SHA-256 verifies the vendored xterm assets |

### Security invariants, as code

- The `Host` allow-list runs as the outermost middleware, before routing; anything
  else gets `421 Misdirected Request` (DNS-rebinding defence).
- Every response passes through the security-headers layer: a strict CSP,
  `X-Frame-Options: DENY`, `nosniff`, `no-referrer`, a `Permissions-Policy`, and
  `Cache-Control: no-store` on HTML and `/api/*`.
- Every route except `/health`, `/pair` and `/assets/*` goes through the `Authed`
  extractor (cookie session, or `Authorization: Bearer` for curl and tests).
- Mutating REST routes and the WebSocket upgrade additionally require a same-site
  `Origin`, checked by a `SameSiteOrigin` extractor that runs before the upgrade is even
  negotiated; mutating REST also requires `X-Spirit-Remote: 1`.
- The access token lives in secure storage, never in `settings.toml`, and has no
  `Display` impl — it escapes only through `AccessToken::reveal()`, which is
  greppable. Sessions are persisted as SHA-256 hashes.
- Pairing failures are rate-limited to 5 per minute per remote address.

Each of these has a test in `app/src/remote_control/mod_tests.rs`, which stands up a
real listener and issues real HTTP requests.

## Decisions honoured

D1 (separate module and crate; `local_control` untouched), D2 (private two-worker tokio
runtime dropped on stop), D3 (loopback by default, `0.0.0.0` under `allow_lan_access`,
no port hunting), D4 (one long-lived token, cookie sessions, 303 redirect so the token
leaves the address bar), D5 (host allow-list, origin checks, strict CSP, no CORS), D6
(plain HTTP with a LAN warning), D7 (JSON control frames, binary PTY frames, full
snapshots with a monotonic version), D8 (raw PTY relay into xterm.js seeded by a
server-built ANSI snapshot; mode-driven input routing), D9 (desktop-authoritative size;
the client scales its font), D10 (opaque decimal ids formatted exactly as
`local_control`'s metadata does), D11 (see Rollout below), D12 (hand-written ES modules,
vendored pinned xterm.js, no Node in the build), D13 (coalesced, structurally-compared
rebuilds with a 1 Hz reconcile), D14 (commands call the same methods and typed actions
the desktop UI calls), D15 (8 clients), D16 (destructive commands require `confirm`),
D17 (in-page alerts only), D18 (no schema changes), D19 (`safe_info!` for anything that
could carry a secret), D20 (no pane splitting).

## Rollout

D11 asks for `FeatureFlag::RemoteControl` to mirror `AdeWorkspaces`, and it does: the
`remote_control` cargo feature is listed in `default` in `app/Cargo.toml`, and
`app/src/features.rs:250-251` adds the flag to `enabled_features()` whenever that cargo
feature is compiled, with no channel check.

**The flag is therefore on in every channel — release, preview, dev and OSS — not just
dogfood.** The `DOGFOOD_FLAGS` entry D11 also calls for is redundant on top of that; it
gates nothing. This is deliberate on the spec's part: the Oss channel is not a dogfood
channel, so `DOGFOOD_FLAGS` alone would not have enabled it there.

What that means for a user: the Settings page, nav item, palette entries, tray entry and
status pill are visible to everyone. No listener starts for anyone —
`remote_control_enabled` defaults to `false`, as does `remote_control_allow_lan_access`,
so no port is bound until it is deliberately turned on.

To make it genuinely dogfood-only, drop `"remote_control"` from `default` in
`app/Cargo.toml`; the existing `DOGFOOD_FLAGS` entry then becomes the real gate.

## One code path, not two

`worktree_sections_for_bindings` was extracted into
`app/src/workspace/view/worktrees.rs` and is now called by both the vertical tabs
renderer and the Remote Control projection, so the sections a phone shows can never
drift from the ones the desktop shows.

## Verification

Automated:

- `crates/remote_control` — 35 unit tests (token generation and alphabet, constant-time
  comparison, session hashing, device labels, host and origin matching including IPv6
  brackets and trailing dots, JSON golden strings for every message shape, command-name
  round-trips, the `ErrorCode` → HTTP status table).
- `crates/warp_terminal::model::ansi_export` — 17 tests (named/bright/indexed/true colour,
  attribute transitions, style carry-over across rows, wide-char spacers, DECSET output,
  cursor positioning).
- `app/src/remote_control/mod_tests.rs` — a real listener on an ephemeral port exercising
  every security invariant end to end.
- `app/src/remote_control/{sessions,assets,lan,projection,bridge,commands,qr}_tests.rs`.
- `app/src/settings/{remote_control,remote_control_secrets}_tests.rs`.
- The web client was validated with a jsdom harness driving the real app against a
  scripted socket (all five screens, attach → snapshot → resize, all three composers,
  the key toolbar's binary frames, resync, sheets, the directory picker, clone progress,
  reconnect, protocol mismatch, and that every destructive command carries `confirm`).

Not done, and the reason:

- **Manual GUI QA on the device matrix** (iPhone Safari, Android Chrome, iPad, macOS
  Safari and Chrome). This work was carried out in a headless Linux container with no
  display and no browser to point at a running Spirit, so no screen of the web client has
  been seen rendered. This is the single largest verification gap and the spec makes it
  mandatory, so it must happen before this ships.
- **Throughput and idle-CPU budgets**, for the same reason.

## Known gaps

- The QR code is served from the app (`GET /api/v1/pairing-qr.svg`, authenticated) and
  shown in the *web* client's settings screen, so an already-paired laptop can pair a
  phone. The desktop Settings page shows the LAN URLs as text but does not yet render the
  code itself; that needs an image widget in the GPU settings page.
- `hello.limits` is a `[{name, value}]` list rather than the object `protocol.html`
  sketches. The client treats `hello` as opaque apart from the version fields, so either
  shape works, but the reference page should be corrected to match the Rust.
- `screen.activate`, `app.ping` and `terminal.agent_insert` are implemented and tested but
  unused by the shipped client (`tab.activate` already activates the owning screen, and
  the key toolbar sends `^C`/`^D` as raw bytes so Ctrl-C cancellation tracking still sees
  them).
- No LAN IPv6, no TLS, no Web Push — all deliberate per D3, D6 and D17.
- `fs.list_dirs` returns `{path, parent, entries}` but not the `suggested` list of recent
  clone/create parents that Phase 8 Task 2 sketches.
- Clone progress frames are queued through an unbounded channel while the terminal
  `starting`/`done`/`failed` phases are sent directly, so a `cloning` frame can in
  principle arrive after `done`. The client keys off the terminal phase, so this is
  cosmetic.
- `worktree.delete` opens the project's screen if it is closed, because the deletion
  pipeline is a `Workspace` method. `worktree.create` does the same, which the spec
  calls for; for `delete` it is an implementation consequence rather than a spec
  requirement.

## Pre-existing breakage found along the way

Neither is caused by this work; both block `./script/presubmit` on Linux.

1. `app/src/crash_recovery.rs` constructed `LaunchMode::App { args, api_key: None }`, but
   that variant has only an `args` field, so `cargo clippy --tests` and `cargo nextest`
   could not build the `warp` crate at all. Fixed here (one line) because presubmit cannot
   pass otherwise.
2. `crates/warpui/src/windowing/winit/linux/status_item.rs` imports the `image` crate,
   which was only a dev-dependency of `warpui`. Reported separately rather than fixed
   here, to keep this change to the feature.
3. `ksni 0.3.6` calls `zvariant::Value::try_into_owned`, which no longer exists, so
   `crates/warpui` cannot compile on Linux at all. Worked around locally in the extracted
   registry source; nothing in the repository was changed for it.
