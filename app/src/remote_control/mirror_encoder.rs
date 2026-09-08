use std::sync::Arc;

use image::ImageEncoder as _;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use remote_control::protocol::{
    MIRROR_FRAME_KEY, MIRROR_FRAME_PATCH, MIRROR_FRAME_STATE, MIRROR_FRAME_VERSION,
    MIRROR_STATE_LIVE, MIRROR_STATE_UNAVAILABLE,
};
use tokio::sync::{mpsc, watch};
use warpui::ModelSpawner;
use warpui::platform::{CapturedFrame, CapturedFrameFormat};

use super::bridge::{ClientId, RemoteControlBridge};

const ROW_GAP_MERGE: u32 = 8;
const KEYFRAME_AREA_RATIO: f64 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
pub(crate) struct MirrorFrame {
    pub frame: Arc<CapturedFrame>,
    pub crop: CropRect,
    pub seq: u64,
}

pub(crate) type MirrorPayload = (u32, Vec<u8>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MirrorState {
    Live,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Rect {
    fn area(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

struct RgbImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbImage {
    fn full_rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        }
    }

    fn row(&self, row: u32) -> &[u8] {
        let stride = self.width as usize * 3;
        let start = row as usize * stride;
        &self.pixels[start..start + stride]
    }

    fn sub_pixels(&self, rect: Rect) -> Vec<u8> {
        let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize * 3);
        for row in rect.y..rect.y + rect.height {
            let line = self.row(row);
            let start = rect.x as usize * 3;
            out.extend_from_slice(&line[start..start + rect.width as usize * 3]);
        }
        out
    }
}

#[derive(Default)]
pub(crate) struct FrameEncoder {
    previous: Option<RgbImage>,
    keyframe_pending: bool,
}

impl FrameEncoder {
    pub fn force_keyframe(&mut self) {
        self.keyframe_pending = true;
    }

    pub fn encode(&mut self, frame: &MirrorFrame) -> Option<Vec<u8>> {
        let Some(current) = crop_rgb(&frame.frame, frame.crop) else {
            self.keyframe_pending = true;
            return None;
        };
        let rects = match &self.previous {
            Some(previous)
                if !self.keyframe_pending
                    && previous.width == current.width
                    && previous.height == current.height =>
            {
                changed_rects(previous, &current)
            }
            Some(_) | None => vec![current.full_rect()],
        };
        if rects.is_empty() {
            return None;
        }
        let full = current.full_rect();
        let changed_area: u64 = rects.iter().map(|rect| rect.area()).sum();
        let (kind, rects) = if changed_area as f64 > full.area() as f64 * KEYFRAME_AREA_RATIO {
            (MIRROR_FRAME_KEY, vec![full])
        } else {
            (MIRROR_FRAME_PATCH, rects)
        };
        let payload = match build_payload(kind, frame.seq, &current, &rects) {
            Ok(payload) => payload,
            Err(_) => {
                self.keyframe_pending = true;
                return None;
            }
        };
        self.previous = Some(current);
        self.keyframe_pending = false;
        Some(payload)
    }
}

fn crop_rgb(frame: &CapturedFrame, crop: CropRect) -> Option<RgbImage> {
    if frame.data.len() != frame.width as usize * frame.height as usize * 4
        || crop.x >= frame.width
        || crop.y >= frame.height
    {
        return None;
    }
    let crop = CropRect {
        x: crop.x,
        y: crop.y,
        width: crop.width.min(frame.width - crop.x),
        height: crop.height.min(frame.height - crop.y),
    };
    if crop.width == 0 || crop.height == 0 {
        return None;
    }
    let mut pixels = Vec::with_capacity(crop.width as usize * crop.height as usize * 3);
    for row in crop.y..crop.y + crop.height {
        let start = (row as usize * frame.width as usize + crop.x as usize) * 4;
        let line = &frame.data[start..start + crop.width as usize * 4];
        for pixel in line.chunks_exact(4) {
            match frame.format {
                CapturedFrameFormat::Rgba => pixels.extend_from_slice(&pixel[..3]),
                CapturedFrameFormat::Bgra => {
                    pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0]])
                }
            }
        }
    }
    Some(RgbImage {
        width: crop.width,
        height: crop.height,
        pixels,
    })
}

fn changed_rects(previous: &RgbImage, current: &RgbImage) -> Vec<Rect> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for row in 0..current.height {
        if previous.row(row) == current.row(row) {
            continue;
        }
        match runs.last_mut() {
            Some((_, end)) if row - *end <= ROW_GAP_MERGE + 1 => *end = row,
            Some(_) | None => runs.push((row, row)),
        }
    }
    runs.into_iter()
        .filter_map(|(start, end)| {
            let mut min_col = u32::MAX;
            let mut max_col = 0;
            for row in start..=end {
                let before = previous.row(row);
                let after = current.row(row);
                for (column, (old, new)) in before
                    .chunks_exact(3)
                    .zip(after.chunks_exact(3))
                    .enumerate()
                {
                    if old != new {
                        min_col = min_col.min(column as u32);
                        max_col = max_col.max(column as u32);
                    }
                }
            }
            (min_col != u32::MAX).then_some(Rect {
                x: min_col,
                y: start,
                width: max_col - min_col + 1,
                height: end - start + 1,
            })
        })
        .collect()
}

fn build_payload(
    kind: u8,
    seq: u64,
    image: &RgbImage,
    rects: &[Rect],
) -> Result<Vec<u8>, image::ImageError> {
    let mut out = Vec::new();
    out.push(MIRROR_FRAME_VERSION);
    out.push(kind);
    out.extend_from_slice(&(seq as u32).to_le_bytes());
    out.extend_from_slice(&(image.width as u16).to_le_bytes());
    out.extend_from_slice(&(image.height as u16).to_le_bytes());
    out.extend_from_slice(&(rects.len() as u16).to_le_bytes());
    for rect in rects {
        let png = encode_png(&image.sub_pixels(*rect), rect.width, rect.height)?;
        out.extend_from_slice(&(rect.x as u16).to_le_bytes());
        out.extend_from_slice(&(rect.y as u16).to_le_bytes());
        out.extend_from_slice(&(rect.width as u16).to_le_bytes());
        out.extend_from_slice(&(rect.height as u16).to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&png);
    }
    Ok(out)
}

fn encode_png(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, image::ImageError> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub).write_image(
        pixels,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(png)
}

pub(crate) fn state_payload(state: MirrorState, message: &str) -> Vec<u8> {
    let message = message.as_bytes();
    let length = message.len().min(usize::from(u16::MAX));
    let mut out = Vec::with_capacity(9 + length);
    out.push(MIRROR_FRAME_VERSION);
    out.push(MIRROR_FRAME_STATE);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(match state {
        MirrorState::Live => MIRROR_STATE_LIVE,
        MirrorState::Unavailable => MIRROR_STATE_UNAVAILABLE,
    });
    out.extend_from_slice(&(length as u16).to_le_bytes());
    out.extend_from_slice(&message[..length]);
    out
}

pub(crate) async fn run_encoder(
    mut frames: watch::Receiver<Option<MirrorFrame>>,
    payloads: mpsc::Sender<MirrorPayload>,
    spawner: ModelSpawner<RemoteControlBridge>,
    client_id: ClientId,
    mirror_id: u32,
) {
    let mut encoder = FrameEncoder::default();
    loop {
        if frames.changed().await.is_err() {
            return;
        }
        let Some(frame) = frames.borrow_and_update().clone() else {
            continue;
        };
        let Ok((returned, encoded)) = tokio::task::spawn_blocking(move || {
            let encoded = encoder.encode(&frame);
            (encoder, encoded)
        })
        .await
        else {
            return;
        };
        encoder = returned;
        if let Some(payload) = encoded {
            match payloads.try_send((mirror_id, payload)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => encoder.force_keyframe(),
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            }
        }
        let reported = spawner
            .spawn(move |bridge, ctx| bridge.mirror_frame_done(client_id, mirror_id, ctx))
            .await;
        if reported.is_err() {
            return;
        }
    }
}

#[cfg(test)]
#[path = "mirror_encoder_tests.rs"]
mod tests;
