use std::sync::Arc;

use remote_control::protocol::{
    MIRROR_FRAME_KEY, MIRROR_FRAME_PATCH, MIRROR_FRAME_STATE, MIRROR_FRAME_VERSION,
    MIRROR_STATE_UNAVAILABLE,
};
use warpui::platform::CapturedFrame;

use super::{CropRect, FrameEncoder, MirrorFrame, MirrorState, state_payload};

struct Decoded {
    kind: u8,
    seq: u32,
    width: u16,
    height: u16,
    rects: Vec<(u16, u16, u16, u16, Vec<u8>)>,
}

fn decode(payload: &[u8]) -> Decoded {
    let u16_at = |offset: usize| u16::from_le_bytes([payload[offset], payload[offset + 1]]);
    let u32_at = |offset: usize| {
        u32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ])
    };
    assert_eq!(payload[0], MIRROR_FRAME_VERSION);
    let count = u16_at(10);
    let mut offset = 12;
    let mut rects = Vec::new();
    for _ in 0..count {
        let rect = (
            u16_at(offset),
            u16_at(offset + 2),
            u16_at(offset + 4),
            u16_at(offset + 6),
        );
        let length = u32_at(offset + 8) as usize;
        let png = payload[offset + 12..offset + 12 + length].to_vec();
        let image = image::load_from_memory(&png).unwrap().into_rgb8();
        assert_eq!(image.dimensions(), (u32::from(rect.2), u32::from(rect.3)));
        rects.push((rect.0, rect.1, rect.2, rect.3, image.into_raw()));
        offset += 12 + length;
    }
    assert_eq!(offset, payload.len());
    Decoded {
        kind: payload[1],
        seq: u32_at(2),
        width: u16_at(6),
        height: u16_at(8),
        rects,
    }
}

fn frame(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3], seq: u64) -> MirrorFrame {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = paint(x, y);
            data.extend_from_slice(&[b, g, r, 255]);
        }
    }
    MirrorFrame {
        frame: Arc::new(CapturedFrame::new_bgra(width, height, data)),
        crop: CropRect {
            x: 0,
            y: 0,
            width,
            height,
        },
        seq,
    }
}

#[test]
fn the_first_frame_is_an_opaque_rgb_keyframe() {
    let mut encoder = FrameEncoder::default();
    let payload = encoder
        .encode(&frame(3, 2, |x, y| [x as u8 * 10, y as u8 * 20, 7], 5))
        .unwrap();
    let decoded = decode(&payload);
    assert_eq!(decoded.kind, MIRROR_FRAME_KEY);
    assert_eq!(decoded.seq, 5);
    assert_eq!((decoded.width, decoded.height), (3, 2));
    assert_eq!(decoded.rects.len(), 1);
    let (x, y, width, height, pixels) = &decoded.rects[0];
    assert_eq!((*x, *y, *width, *height), (0, 0, 3, 2));
    assert_eq!(&pixels[..6], &[0, 0, 7, 10, 0, 7]);
}

#[test]
fn crops_the_window_at_the_requested_rectangle() {
    let mut encoder = FrameEncoder::default();
    let mut window = frame(4, 4, |x, y| [x as u8, y as u8, 0], 1);
    window.crop = CropRect {
        x: 1,
        y: 2,
        width: 2,
        height: 1,
    };
    let decoded = decode(&encoder.encode(&window).unwrap());
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(decoded.rects[0].4, vec![1, 2, 0, 2, 2, 0]);
}

#[test]
fn an_unchanged_frame_sends_nothing() {
    let mut encoder = FrameEncoder::default();
    assert!(encoder.encode(&frame(8, 8, |_, _| [1, 2, 3], 1)).is_some());
    assert!(encoder.encode(&frame(8, 8, |_, _| [1, 2, 3], 2)).is_none());
}

#[test]
fn a_small_change_becomes_a_patch_of_the_changed_rectangle() {
    let mut encoder = FrameEncoder::default();
    assert!(
        encoder
            .encode(&frame(40, 40, |_, _| [0, 0, 0], 1))
            .is_some()
    );
    let changed = frame(
        40,
        40,
        |x, y| {
            if (5..8).contains(&x) && y == 9 {
                [255, 0, 0]
            } else {
                [0, 0, 0]
            }
        },
        2,
    );
    let decoded = decode(&encoder.encode(&changed).unwrap());
    assert_eq!(decoded.kind, MIRROR_FRAME_PATCH);
    assert_eq!(decoded.rects.len(), 1);
    let (x, y, width, height, pixels) = &decoded.rects[0];
    assert_eq!((*x, *y, *width, *height), (5, 9, 3, 1));
    assert_eq!(pixels, &[255, 0, 0, 255, 0, 0, 255, 0, 0]);
}

#[test]
fn distant_changes_become_separate_patches() {
    let mut encoder = FrameEncoder::default();
    assert!(
        encoder
            .encode(&frame(40, 40, |_, _| [0, 0, 0], 1))
            .is_some()
    );
    let changed = frame(
        40,
        40,
        |x, y| {
            if (y == 0 || y == 39) && x == 0 {
                [9, 9, 9]
            } else {
                [0, 0, 0]
            }
        },
        2,
    );
    let decoded = decode(&encoder.encode(&changed).unwrap());
    assert_eq!(decoded.kind, MIRROR_FRAME_PATCH);
    assert_eq!(decoded.rects.len(), 2);
    assert_eq!(decoded.rects[0].1, 0);
    assert_eq!(decoded.rects[1].1, 39);
}

#[test]
fn a_large_change_promotes_to_a_keyframe() {
    let mut encoder = FrameEncoder::default();
    assert!(
        encoder
            .encode(&frame(10, 10, |_, _| [0, 0, 0], 1))
            .is_some()
    );
    let decoded = decode(&encoder.encode(&frame(10, 10, |_, _| [1, 1, 1], 2)).unwrap());
    assert_eq!(decoded.kind, MIRROR_FRAME_KEY);
}

#[test]
fn a_resize_or_forced_keyframe_resends_everything() {
    let mut encoder = FrameEncoder::default();
    assert!(encoder.encode(&frame(4, 4, |_, _| [0, 0, 0], 1)).is_some());
    let decoded = decode(&encoder.encode(&frame(5, 4, |_, _| [0, 0, 0], 2)).unwrap());
    assert_eq!(decoded.kind, MIRROR_FRAME_KEY);
    assert_eq!(decoded.width, 5);
    encoder.force_keyframe();
    let decoded = decode(&encoder.encode(&frame(5, 4, |_, _| [0, 0, 0], 3)).unwrap());
    assert_eq!(decoded.kind, MIRROR_FRAME_KEY);
}

#[test]
fn a_crop_past_the_window_edge_is_clamped() {
    let mut encoder = FrameEncoder::default();
    let mut window = frame(2, 2, |x, _| [x as u8, 0, 0], 1);
    window.crop = CropRect {
        x: 1,
        y: 0,
        width: 2,
        height: 2,
    };
    let decoded = decode(&encoder.encode(&window).unwrap());
    assert_eq!((decoded.width, decoded.height), (1, 2));
    assert_eq!(decoded.rects[0].4, vec![1, 0, 0, 1, 0, 0]);
}

#[test]
fn a_crop_outside_the_window_is_skipped() {
    let mut encoder = FrameEncoder::default();
    let mut window = frame(2, 2, |_, _| [0, 0, 0], 1);
    window.crop = CropRect {
        x: 2,
        y: 0,
        width: 1,
        height: 1,
    };
    assert!(encoder.encode(&window).is_none());
}

#[test]
fn state_frames_carry_the_message() {
    let payload = state_payload(MirrorState::Unavailable, "hidden");
    assert_eq!(payload[0], MIRROR_FRAME_VERSION);
    assert_eq!(payload[1], MIRROR_FRAME_STATE);
    assert_eq!(payload[6], MIRROR_STATE_UNAVAILABLE);
    assert_eq!(u16::from_le_bytes([payload[7], payload[8]]), 6);
    assert_eq!(&payload[9..], b"hidden");
}
