use base64::Engine as _;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::vec2f;
use warpui::platform::CapturedFrame;

use super::encode_frame;

#[test]
fn captures_only_the_terminal_at_the_window_scale() {
    let frame = CapturedFrame::new_bgra(
        3,
        2,
        vec![
            0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 30, 20, 10, 255, 60, 50, 40,
            255,
        ],
    );
    let result = encode_frame(
        frame,
        RectF::new(vec2f(0.5, 0.5), vec2f(1.0, 0.5)),
        2.0,
        None,
    )
    .unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(result["image"].as_str().unwrap())
        .unwrap();
    let image = image::load_from_memory(&png).unwrap().into_rgba8();
    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image.into_raw(), vec![10, 20, 30, 255, 40, 50, 60, 255]);
}

#[test]
fn rejects_a_crop_outside_the_rendered_window() {
    let frame = CapturedFrame::new(1, 1, vec![0, 0, 0, 255]);
    assert!(
        encode_frame(
            frame,
            RectF::new(vec2f(1.0, 0.0), vec2f(1.0, 1.0)),
            1.0,
            None
        )
        .is_err()
    );
}

#[test]
fn rejects_incomplete_pixel_data() {
    let frame = CapturedFrame::new(2, 2, vec![0, 0, 0, 255]);
    assert!(
        encode_frame(
            frame,
            RectF::new(vec2f(0.0, 0.0), vec2f(1.0, 1.0)),
            1.0,
            None
        )
        .is_err()
    );
}

#[test]
fn unchanged_frames_do_not_resend_pixels() {
    let frame = CapturedFrame::new(1, 1, vec![10, 20, 30, 255]);
    let bounds = RectF::new(vec2f(0.0, 0.0), vec2f(1.0, 1.0));
    let first = encode_frame(frame.clone(), bounds, 1.0, None).unwrap();
    let next = encode_frame(frame, bounds, 1.0, first["fingerprint"].as_str()).unwrap();
    assert!(next["image"].is_null());
    assert_eq!(next["fingerprint"], first["fingerprint"]);
}
