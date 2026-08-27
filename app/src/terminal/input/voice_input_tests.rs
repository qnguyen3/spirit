use warpui::platform::keyboard::KeyCode;

use super::{VoiceInputState, hold_key, hold_key_label};

#[test]
fn hold_key_is_a_modifier_so_it_cannot_collide_with_typed_characters() {
    // AltRight is AltGr on many non-US layouts, where holding it composes characters such
    // as `@` and `€`, so it must never be the hold key.
    assert!(matches!(hold_key(), KeyCode::Fn | KeyCode::ControlRight));
}

#[test]
fn idle_tooltip_advertises_the_key_the_handler_listens_for() {
    assert!(VoiceInputState::Idle.tooltip().contains(hold_key_label()));
}

#[test]
fn listening_tooltip_offers_to_stop_rather_than_repeating_the_hold_key() {
    let tooltip = VoiceInputState::Listening.tooltip();
    assert!(!tooltip.contains(hold_key_label()));
    assert!(tooltip.contains("Stop"));
}
