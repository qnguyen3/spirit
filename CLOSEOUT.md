# Spirit — Oz Stripping & Agents Enhancement: Closeout

Closeout for the effort specced in `spirit_specs/0826-oz-stripping-and-agents-enhancement/`.
Baseline: commit `60d602df` (2026-08-26). Verified: 2026-08-27.

## Headline numbers

- 739,235 lines deleted, 24,569 added across 1,893 files (Rust alone: 695,503 deleted,
  24,028 added) over 53 commits.
- 11 crates removed: `ai`, `ai_types`, `build_cache`, `computer_use`, `field_mask`,
  `input_classifier`, `mcp`, `natural_language_detection`, `voice_input`,
  `warp_multi_agent_client`, `warp_tui`. The workspace now has 69 member crates.
- The external `warp_multi_agent_api` protobuf git dependency is gone from the
  dependency tree, as are `rmcp`, `ort`, `candle`, and `cpal` (51 MB of ONNX model
  assets went with `input_classifier`).
- `FeatureFlag::AgentLauncher` (the Phase 1 bring-up flag) is removed; the launcher is
  unconditional.

## Verification results

All run locally on macOS, 2026-08-27, in presubmit's configuration:

- `./script/format --check` and the inline-test-module check: clean.
- Clippy trio (`--workspace --exclude warp_completer --all-targets --tests`,
  `-p warp`, `-p warp_completer` default features, all `-D warnings`): green.
  Note: `--all-features` clippy fails on pre-existing lints in `warp_completer`'s
  WIP v2 feature code, untouched by this effort; presubmit deliberately excludes it.
- `cargo nextest run --no-fail-fast --workspace --exclude command-signatures-v2`:
  6,256 of 6,261 passed on the first full run. The 5 failures are the SSH
  integration tests (`test_ssh_into_ash`, `test_ssh_into_sh`,
  `test_ssh_wrapper_into_bash`, `test_ssh_wrapper_into_zsh`,
  `test_ssh_with_shell_override`), which tunnel through GCP IAP into the private
  `warp-ssh-integration-testing` project and cannot run without that access. On the
  post-cleanup re-run (SSH excluded), 6,237 of 6,238 passed;
  `shell_integration_tests::test_ctrl_c` failed under parallel build load and passes
  in isolation (flaky, unrelated).
- `cargo nextest run -p warp_completer --features v2` and `cargo test --doc`: green.
- `run-clang-format.py` and `wgslfmt` are not installed on this machine; no C/C++/
  Obj-C or WGSL files were touched by Phase 6.
- Per-channel binaries (`oss`, `local`, `stable`, `dev`, `preview`, `integration`)
  covered by the all-targets clippy pass; `warp-oss` built and smoke-launched (GUI
  stays alive, no panic output).
- `dump-settings-schema` output audited: no dead AI settings remain. The legacy
  `agent`/`cloud_agent` stored values are documented as historical and map to
  `Terminal`. Remaining agent-named settings (`is_agent_task_completed_enabled`,
  `auto_open_code_review_pane_on_first_agent_change`, vertical-tab prompt titles)
  all have live consumers in the kept CLI-agent session UX.
- Keybinding audit: the only agent binding is the launcher's own
  `workspace:new_agent_picker`.
- CLI: `warp agent run` (and `mcp`, `run`, `runner`, `schedule`, `harness-support`)
  print "not supported in Spirit. Launch a third-party CLI agent with New Agent
  instead."; these subcommands are now also hidden from `--help`.
- Straggler greps (`crate::ai`, AI crate names in TOML, `dangerously` outside the
  launcher catalog, AI crates under `crates/`): all clean.

## Accepted survivors

- `crates/warp_cli` (the Oz CLI argument surface): kept as the shared CLI parsing
  layer. `warpctrl`/local-control, shell completions, workers, and the terminal's CLI
  plumbing use it. Agent-facing subcommands (`agent`, `mcp`, `run`, `runner`,
  `schedule`, `harness-support`) are rejected at dispatch in `app/src/lib.rs` with
  "not supported in Spirit. Launch a third-party CLI agent with New Agent instead."
- `crates/remote_server` Oz CLI tarball installer and `crates/managed_secrets`
  `oz federate` plumbing: SSH remote-server infrastructure, independent of the in-app
  agent.
- `crates/cloud_object_models` agent-era models (`scheduled_ambient_agent`,
  `cloud_environment`, `AgentConfigSnapshot`): server data models kept so Warp Drive
  sync tolerates objects other clients still create (decision D2), taken as typed
  models rather than skip-on-unknown.
- Legacy pane-kind constants (`AI_FACT_PANE_KIND` etc.) in `crates/persistence` and
  `app/src/persistence/sqlite.rs`: required to restore pre-Spirit sessions — panes
  saved by older builds for removed AI features restore as fresh terminal panes
  (decision D4). Likewise `PaneMode::Agent`/`PaneMode::Cloud` in launch configs parse
  legacy configs and open terminals.
- `agent_view` cargo feature / `FeatureFlag::AgentView` (and dependents
  `inline_history_menu`, `inline_slash_commands`, `inline_model_selector`): legacy
  names that now gate the modern universal terminal input UX. Load-bearing, default-on;
  renaming is cosmetic and deferred.
- `crates/onboarding` `agent_onboarding_view.rs` / `AgentOnboardingView`: legacy name
  for the general onboarding flow. Its slides are intro, theme picker, customize, and
  offer — no agent content (decision D6 holds).
- Integration test `test_secrets_are_always_redacted_in_ai_inputs`: live test of
  terminal secret redaction; only the name is stale.
- `app/src/terminal/cli_agent_sessions/` and `terminal/cli_agent.rs`: deliberately kept
  (decision D3); the launcher catalog keys off `CLIAgent`.
- Remaining `oz`/`Oz` strings sit in `warp_cli` help text, remote-server installer
  paths, log directory names, and feature-flag doc comments tied to the surviving CLI
  parsing layer.

## Deviations from the spec

- Voice input: the spec deleted `crates/voice_input`; a new, self-contained voice input
  feature was later added on purpose (`FeatureFlag::VoiceInput`, dogfood-only). Not a
  strip regression.
- `warp agent run` yields a clear "not supported in Spirit" error rather than an
  unknown-command error, because `warp_cli` parsing is retained.
- D2 tolerance is implemented by keeping typed server models instead of skipping
  unknown object kinds.
- Phase 6 additionally removed dead settings chains that earlier phases orphaned:
  the "Agent font" appearance widget and `Appearance::ai_font_family` plumbing,
  `AIFontName`/`MatchAIFontToTerminalFont`, the codebase-indexing settings
  (`agent_mode_codebase_context*`), `ShowAgentTips` plus `FeatureFlag::AgentTips`, and
  their context flags — none had a consumer left. It also hid the rejected Oz
  subcommands from CLI `--help`.

## Not verified here

- Binary size and clean-build time before/after were not measured (no pre-strip build
  retained on this machine).
- Windows and Linux builds, the wasm session viewer against a live shared session, and
  the release bundle scripts' `--check-only` mode still need a CI pass on those
  platforms.

---

# Spirit — Warp-Server Stripping: Closeout

Closeout for the effort specced in `spirit_specs/0831-warp-server-stripping/`.
Baseline: commit `b9df5e75` (2026-08-31). Verified: 2026-09-01.

## Headline numbers

- 230,336 lines deleted, 2,907 added across 1,058 files (Rust alone: 217,621 deleted,
  2,625 added) over 11 phases.
- 14 crates removed: `warp_server_client`, `warp_server_auth`, `graphql`
  (`warp_graphql`), `warp_graphql_schema`, `cloud_objects`, `cloud_object_client`,
  `cloud_object_models`, `cloud_object_persistence`, `managed_secrets`,
  `managed_secrets_wasm`, `firebase`, `warp_web_event_bus`, plus
  `app-installation-detection` and the session-viewer web build from Phase 1. The
  workspace went from 68 member crates to 54.
- External dependencies gone from the tree: `cynic`, `cynic-codegen`,
  `graphql-ws-client`, `oauth2`, `reqwest-eventsource`, `session-sharing-protocol`,
  `sentry` + `sentry-log`, the OpenTelemetry stack (`opentelemetry`,
  `opentelemetry-http`, `opentelemetry-otlp`, `opentelemetry_sdk`,
  `tracing-opentelemetry`, `tracing-subscriber`), `mockito`, and the
  `tink-core`/`tink-proto`/`tink-hybrid` git patches. `Cargo.lock` lost 909 lines.
- No warp.dev URL, Firebase API key, RudderStack write key, or Sentry DSN is compiled
  into the binary any more: `WarpServerConfig`, `OzConfig` and `IapConfig` are gone
  from the channel config entirely.
- A new migration, `2026-09-01-000000_drop_warp_server_tables`, drops 19 orphaned
  tables. A fresh database now has 21 tables (plus diesel's bookkeeping), down from 41.

## Verification results

All run locally on macOS, 2026-09-01, in presubmit's configuration:

- `./script/format --check` and the inline-test-module check: clean.
- Clippy trio (`--workspace --exclude warp_completer --all-targets --tests`,
  `-p warp`, `-p warp_completer`, all `-D warnings`): green.
- `cargo nextest run --no-fail-fast --workspace --exclude command-signatures-v2`:
  5,270 tests, 5,260 passed, 10 failed. The 10 are the unchanged pre-existing
  baseline: 5 SSH tests that tunnel through GCP IAP into the private
  `warp-ssh-integration-testing` project, and 5 tab-bar/settings-chrome tests
  (`test_add_and_close_session`, `test_open_and_close_settings`,
  `test_removing_tabs_out_of_order`, `test_undo_close_stack_timeout_cleanup`,
  `test_tab_context_menu_copies_metadata`) that fail on "No position for
  new_tab_button" / "close_tab_button:1" — fallout from `b9df5e75 settings as pinned
  tab`, present before this effort began and identical after every phase.
- With SSH excluded (`-E 'not test(/ssh/)'`): 5,247 run, 5,242 passed, the same 5
  chrome failures.
- `cargo nextest run -p warp_completer --features v2` (128 passed) and
  `cargo test --doc`: green.
- `./script/test_factory_files_skill.py` and `./script/check_license_config_sync`:
  green.
- `warp-oss` builds and links.
- Suite size fell from 6,164 to 5,270 — a larger drop than the spec's ~450 estimate,
  fully accounted for by Phase 9 deleting whole crates with their own suites
  (`warp_server_client` 49, `cloud_object_models` 29, `warp_server_auth` 4,
  `warp_graphql` 4, `cloud_object_persistence` 4) plus 142 `warp_cli` subcommand
  tests. Phases 1–8 together account for the other ~530.
- Migration verified by hand: replayed the 142 pre-existing migrations onto an empty
  database, seeded a window carrying `team_uid` and `warp_drive_index_width` with five
  tabs (workflow pane, cloud notebook pane, env-var pane, local `.md` notebook pane,
  settings pane on the `Account` slug) plus `teams`/`team_members`/`users` rows, then
  ran the new migration. Every dropped table is gone; `blocks`, `commands`, `tabs`,
  `windows`, `projects`, `project_worktrees`, `workspace_metadata`,
  `workspace_language_server` and `ignored_suggestions` are intact with their data;
  the workflow and env-var leaves and their orphaned `pane_nodes` are removed; the
  local `.md` pane keeps its path; the cloud notebook keeps its row and restores as a
  fresh terminal. `diesel migration run` from empty then `diesel print-schema` diffs
  clean against `schema.rs`, which is what CI's `database-migration` job checks.

## Not verified here

- **Network silence was not observed with a packet monitor.** The evidence here is
  static: no warp.dev URL survives in the channel config or any Rust source, every
  GraphQL/websocket/SSE client crate is out of the dependency tree, and the app builds
  and links without them. A `nettop`/Little Snitch session and a Wi-Fi-off run are
  still worth doing before shipping.
- The manual functional sweep (terminal core, workflows, notebooks, ADE, settings
  pages, SSH against a locally deployed remote server, menu/palette walk) was not
  performed — only the automated suite and the migration replay.
- Windows and Linux builds, and the per-channel release-bundle matrix, still need a CI
  pass. Only `warp-oss` was built locally.
- Cold-build time and binary size before/after were not measured (no pre-strip build
  retained on this machine).

## Accepted survivors

- **Inert `sync_to_cloud` setting metadata** (247 occurrences): every setting still
  declares whether it *would* sync. Nothing reads it now, but stripping the field
  would touch every settings definition for no behavioural gain.
- **Legacy pane-kind constants** (`WORKFLOW_PANE_KIND`, `ENV_VAR_COLLECTION_PANE_KIND`,
  `AI_FACT_PANE_KIND`, …) in `crates/persistence/src/model.rs`: they map old panes to
  fresh terminal panes on restore (decision D4), and the migration deliberately leaves
  the mapping reachable for databases that skip it.
- **Orphaned SQLite columns**, per the migration policy (never `DROP COLUMN`):
  `commands.cloud_workflow_id`, `windows.team_uid`, `windows.warp_drive_index_width`,
  `notebook_panes.notebook_id`. `windows.voltron_width` is live and untouched.
- **`crates/websocket`** — kept for `remote_tty`; only its `graphql_ws_client` adapter
  was removed. **`crates/http_client`** — kept for autoupdate, changelog, LSP server
  downloads and asset fetching; lost IAP, oauth2 and SSE. **`crates/http_server`** —
  local loopback only (install detection, profiling).
- **`crates/warp_cli`** — kept as the shared CLI parsing layer for `warpctrl`, shell
  completions and the worker subcommands. Every server subcommand and `CliCommand`
  itself are gone.
- **The dormant autoupdate and changelog stack** (D4/D21): `releases.warp.dev` and
  release-asset URLs survive in `app/src/autoupdate/`, `changelog_model.rs` and
  `crates/channel_versions`, but `autoupdate_config` is `None` on every channel this
  fork builds, so nothing renders or fetches. The version and update-status widgets
  moved to the About settings page and stay inert.
- **`#[cfg(target_family = "wasm")]` code** (601 occurrences): the wasm target is no
  longer built, but the cfg-guarded branches are left in place (D5 deferred cleanup).
- **`crates/persistence`'s `firebase_uid`-keyed structs** are gone, but the word
  survives in migration history SQL, which is never edited.
- **`script/test_factory_files_skill.py`'s `WARP_SERVER_ROOT_URL` handling**: it
  exercises the factory-files agent skill's own validator, not app code.

## Deviations from the spec

Full per-phase deviation logs live alongside the specs. The ones that change the
shipped product:

- **Remote-server auto-install is gone, not just its URLs (D10).** Both install paths
  (remote-host download and client-download-then-SCP) sourced their artifact from
  `app.warp.dev/download/cli`. `install_binary` now returns an error naming
  `script/deploy_remote_server`, and the SSH controller falls back to a plain session,
  which is the rsync-deployed workflow D10 assumes.
- **The update-deadline banner uses the local clock.** `ServerTime` came from
  `/current_time` and existed to resist clock tampering; `new_version.update_by` is
  still compared, now against `chrono::Utc::now()`.
- **Settings nav reordered so Appearance is the first row**, since it became the
  default page when Account was removed. Retired slugs (`Account`, `Teams`,
  `Warp Drive`, `Billing and usage`, `Referrals`, `Shared blocks`, `Environments`,
  `Oz Cloud API Keys`) resolve to the default page instead of failing, so old session
  snapshots and `warpctrl surface.settings.open --page` degrade gracefully.
- **Glyph fallback lost its remote font fetch (D16).** `app/src/font_fallback.rs`
  built Hack-Nerd-Font and Noto URLs off the server root; fallback now uses only
  bundled and system fonts. Some CJK, Arabic, Devanagari, Bengali and symbol glyphs
  may render as tofu where the system has no face. **This is the one user-visible
  regression of the whole effort and is worth a follow-up** — bundling the Noto subset
  would close it.
- **Onboarding lost its offer slide and post-auth step**, the intro slide's "Log in"
  row, and the theme slide's "Privacy Settings" link (its only destination was the
  deleted login slide's cloud-storage toggle). The flow is Intro → Customize → Theme,
  and the theme slide's disclaimer keeps only the Terms of Service line.
- **The macOS `ApplePressAndHoldEnabled` override moved from the login screen to the
  onboarding branch**, preserving "held keys repeat" on genuinely first runs.
- **Four dangling `#[cfg(feature = …)]` lines in `app/src/features.rs`** — left by
  earlier phases removing a flag but not its attribute — were silently over-gating the
  *next* flag in the list, including `DefaultAdeberryTheme`. Fixed in Phase 11.
- **Two runtime traps that only tests caught**: `AuthStateProvider::as_ref` still ran
  in `persistence/sqlite.rs` and `remote_server/server_model.rs` after the singleton
  stopped being registered. A `SingletonEntity` read compiles fine and panics at
  startup — grep for `*Provider::as_ref` after deleting a singleton.
