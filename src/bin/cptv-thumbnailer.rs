//! Freedesktop thumbnailer for CPTV files (GNOME Files/Nautilus, Nemo, Caja,
//! Thunar via tumbler). Registered by a `.thumbnailer` file as
//!
//!     cptv-thumbnailer -s %s %i %o
//!
//! and writes a PNG no larger than `size` × `size`.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cptv_native_viewers::palette::Palette;
use cptv_native_viewers::source::{Opener, Reader};
use cptv_native_viewers::{Clip, Load, Normalize, Render};

/// Sensor width; frames are 160×120.
const SENSOR_W: usize = 160;

fn usage() -> ExitCode {
    eprintln!("usage: cptv-thumbnailer [-s SIZE] [-p PALETTE] INPUT.cptv OUTPUT.png");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut size = 256usize;
    let mut palette = Palette::default();
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-s" => match args.next().and_then(|s| s.parse().ok()) {
                Some(s) if s > 0 => size = s,
                _ => return usage(),
            },
            "-p" => match args.next() {
                Some(p) => palette = Palette::from_name(&p),
                None => return usage(),
            },
            "-h" | "--help" => return usage(),
            _ => paths.push(PathBuf::from(a)),
        }
    }
    let [input, output] = paths.as_slice() else {
        return usage();
    };
    match thumbnail(input, output, size, palette) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cptv-thumbnailer: {}: {e}", input.display());
            ExitCode::FAILURE
        }
    }
}

fn thumbnail(input: &Path, output: &Path, size: usize, palette: Palette) -> Result<(), String> {
    // Render at the smallest integer upscale that covers `size`, then shrink
    // to fit exactly (the core's upscaler is smoother than ours).
    let scale = size.div_ceil(SENSOR_W).clamp(1, 16);
    let path = input.to_path_buf();
    let open: Opener = Box::new(move || Ok(Box::new(BufReader::new(File::open(&path)?)) as Reader));
    // Per-frame normalisation never needs a pass over the whole file, and
    // StreamOnly never decodes the whole clip into memory (older files have
    // no frame count in the header; a 10-minute one would take ~200 MB).
    let mut clip = Clip::open_with_load(open, palette, Normalize::Frame, scale, Load::StreamOnly)?;

    // Frames are delta-coded in one gzip stream, so frame N costs N decodes.
    // About a second in skips any start-up frames but stays cheap; a shorter
    // clip gives its last frame (the frame count may be unknown here).
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

    let (tw, th) = fit(w, h, size);
    let rgb = if (tw, th) == (w, h) { rgb } else { shrink(&rgb, w, h, tw, th) };
    write_png(output, &rgb, tw, th)
}

/// Largest size with the same aspect ratio that fits in `size` × `size`
/// (never larger than the source).
fn fit(w: usize, h: usize, size: usize) -> (usize, usize) {
    if w <= size && h <= size {
        return (w, h);
    }
    if w >= h {
        (size, (h * size + w / 2) / w)
    } else {
        ((w * size + h / 2) / h, size)
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

fn write_png(path: &Path, rgb: &[u8], w: usize, h: usize) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("can't create {}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut wr| wr.write_image_data(rgb))
        .map_err(|e| format!("can't write PNG: {e}"))
}
