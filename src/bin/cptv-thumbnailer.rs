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
use cptv_native_viewers::thumbnail;

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
    let path = input.to_path_buf();
    let open: Opener = Box::new(move || Ok(Box::new(BufReader::new(File::open(&path)?)) as Reader));
    let t = thumbnail::render(open, size, palette)?;
    write_png(output, &t.rgb, t.width, t.height)
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
