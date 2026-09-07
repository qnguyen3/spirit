pub const MAX_CLIENTS: u64 = 8;
pub const MAX_ATTACHMENTS_PER_CLIENT: u64 = 4;
pub const MAX_JSON_FRAME_BYTES: u64 = 64 * 1024;
pub const MAX_INPUT_FRAME_BYTES: u64 = 4 * 1024;
pub const MAX_PASTE_BYTES: u64 = 256 * 1024;
pub const MAX_INPUT_BYTES_PER_SECOND: u64 = 64 * 1024;
pub const OUTPUT_COALESCE_BYTES: u64 = 16 * 1024;
pub const OUTPUT_COALESCE_MS: u64 = 8;
pub const OUTPUT_QUEUE_FRAMES: u64 = 256;
pub const HISTORY_SNAPSHOT_MAX_BYTES: u64 = 256 * 1024;
pub const PAIR_FAILURES_PER_MINUTE: u64 = 5;
pub const SESSION_IDLE_DAYS: u64 = 30;
pub const STATE_COALESCE_MS: u64 = 50;
pub const CLIENT_IDLE_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_DIRECTORY_ENTRIES: u64 = 2000;
pub const COOKIE_MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

pub fn all() -> &'static [(&'static str, u64)] {
    &[
        ("max_clients", MAX_CLIENTS),
        ("max_attachments_per_client", MAX_ATTACHMENTS_PER_CLIENT),
        ("max_json_frame_bytes", MAX_JSON_FRAME_BYTES),
        ("max_input_frame_bytes", MAX_INPUT_FRAME_BYTES),
        ("max_paste_bytes", MAX_PASTE_BYTES),
        ("max_input_bytes_per_second", MAX_INPUT_BYTES_PER_SECOND),
        ("output_coalesce_bytes", OUTPUT_COALESCE_BYTES),
        ("output_coalesce_ms", OUTPUT_COALESCE_MS),
        ("output_queue_frames", OUTPUT_QUEUE_FRAMES),
        ("history_snapshot_max_bytes", HISTORY_SNAPSHOT_MAX_BYTES),
        ("pair_failures_per_minute", PAIR_FAILURES_PER_MINUTE),
        ("session_idle_days", SESSION_IDLE_DAYS),
        ("state_coalesce_ms", STATE_COALESCE_MS),
        ("client_idle_timeout_seconds", CLIENT_IDLE_TIMEOUT_SECONDS),
        ("max_directory_entries", MAX_DIRECTORY_ENTRIES),
        ("cookie_max_age_seconds", COOKIE_MAX_AGE_SECONDS),
    ]
}
