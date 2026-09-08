use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::vec2f;

use super::{CropRect, crop_for, typed_chars_for_key};

#[test]
fn named_keys_fall_back_to_their_typed_characters() {
    assert_eq!(typed_chars_for_key("enter"), Some("\r"));
    assert_eq!(typed_chars_for_key("tab"), Some("\t"));
    assert_eq!(typed_chars_for_key("escape"), Some("\x1b"));
    assert_eq!(typed_chars_for_key("backspace"), Some("\x7f"));
    assert_eq!(typed_chars_for_key("up"), None);
    assert_eq!(typed_chars_for_key("a"), None);
}

#[test]
fn crops_scale_logical_bounds_to_backing_pixels() {
    let crop = crop_for(RectF::new(vec2f(0.5, 0.5), vec2f(1.0, 0.5)), 2.0);
    assert_eq!(
        crop,
        CropRect {
            x: 1,
            y: 1,
            width: 2,
            height: 1
        }
    );
}
