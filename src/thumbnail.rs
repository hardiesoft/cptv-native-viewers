//! Thumbnail rendering shared by the file-manager integrations: the Linux
//! freedesktop thumbnailer (`src/bin/cptv-thumbnailer.rs`) and the Windows
//! Explorer thumbnail handler (`windows/thumbnail-handler`). macOS Quick Look
//! draws the same frame through the C API.

use crate::palette::Palette;
use crate::source::Opener;
use crate::{Clip, Load, Normalize, Render};

/// Sensor width; frames are 160×120.
const SENSOR_W: usize = 160;

/// A thumbnail as packed 8-bit RGB, `width * height * 3` bytes, top row first.
pub struct Thumbnail {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

/// Render a thumbnail no larger than `size` × `size` (keeping the 4:3 aspect
/// ratio) of the frame about one second into the clip, or the last frame of a
/// shorter clip.
///
/// Cheap for any file: per-frame normalisation never needs a pass over the
/// whole clip, and [`Load::StreamOnly`] never decodes it all into memory
/// (older files have no frame count in the header; a 10-minute one would
/// otherwise take ~200 MB). Frames are delta-coded in one gzip stream, so
/// frame N costs N decodes, which is why an early frame is used.
pub fn render(open: Opener, size: usize, palette: Palette) -> Result<Thumbnail, String> {
    // Render at the smallest integer upscale that covers `size`, then shrink
    // to fit exactly (the core's upscaler is smoother than a plain resize).
    let scale = size.max(1).div_ceil(SENSOR_W).clamp(1, 16);
    let mut clip = Clip::open_with_load(open, palette, Normalize::Frame, scale, Load::StreamOnly)?;

    let target = clip.fps.round() as usize;
    let (w, h) = (clip.width(), clip.height());
    let mut argb = vec![0u8; w * h * 4];
    if clip.render_clamped(target, &mut argb, w * 4) != Render::Ok {
        return Err("no frames".into());
    }

    // Native-endian 0xAARRGGBB -> RGB.
    let rgb: Vec<u8> = argb
        .chunks_exact(4)
        .flat_map(|p| {
            let v = u32::from_ne_bytes([p[0], p[1], p[2], p[3]]);
            [(v >> 16) as u8, (v >> 8) as u8, v as u8]
        })
        .collect();

    let (tw, th) = fit(w, h, size.max(1));
    let rgb = if (tw, th) == (w, h) { rgb } else { shrink(&rgb, w, h, tw, th) };
    Ok(Thumbnail { width: tw, height: th, rgb })
}

/// Largest size with the same aspect ratio that fits in `size` × `size`
/// (never larger than the source).
fn fit(w: usize, h: usize, size: usize) -> (usize, usize) {
    if w <= size && h <= size {
        return (w, h);
    }
    if w >= h {
        (size, ((h * size + w / 2) / w).max(1))
    } else {
        (((w * size + h / 2) / h).max(1), size)
    }
}

/// Area-average downscale of packed RGB.
fn shrink(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(dw * dh * 3);
    for y in 0..dh {
        let (y0, y1) = (y * sh / dh, ((y + 1) * sh / dh).max(y * sh / dh + 1));
        for x in 0..dw {
            let (x0, x1) = (x * sw / dw, ((x + 1) * sw / dw).max(x * sw / dw + 1));
            let mut sum = [0u32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * sw + sx) * 3;
                    for c in 0..3 {
                        sum[c] += src[i + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            out.extend(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn fit_keeps_aspect_and_never_upscales() {
        assert_eq!(fit(640, 480, 256), (256, 192));
        assert_eq!(fit(160, 120, 256), (160, 120));
        assert_eq!(fit(640, 480, 1), (1, 1));
    }
}
