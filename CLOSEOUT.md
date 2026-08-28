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
