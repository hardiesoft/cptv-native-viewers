//! Rust core of the CPTV VLC plugin.
//!
//! The VLC-facing glue lives in `vlc/cptv_demux.c`; this crate exposes a tiny
//! C ABI (see `vlc/cptv_core.h`) that:
//!
//! * probes a buffer to see whether it looks like a CPTV file,
//! * opens a clip through read/seek callbacks using the upstream `codec`
//!   crate, either streaming frames on demand (when the header says how many
//!   frames there are) or decoding the whole clip into memory (see
//!   [`source`]),
//! * renders any frame to a normalised, colour-mapped, upscaled RV32 image.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{self, Cursor, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::Arc;

use codec::common::{HEIGHT, WIDTH};
use codec::decode::CptvHeader;

pub mod palette;
pub mod source;

use palette::Palette;
use source::{new_decoder, next_playable, Fetch, FrameSource, Opener, StreamSource};

const SRC_W: usize = WIDTH;
const SRC_H: usize = HEIGHT;

/// How raw sensor values are stretched into the 0..1 range before colouring.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Normalize {
    /// Each frame uses its own min/max (maximum contrast, may flicker).
    Frame,
    /// One min/max across the whole clip (stable brightness).
    Clip,
}

/// How much of a clip may be decoded when it's opened.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Load {
    /// Stream when the header has `total_frames`; otherwise decode the whole
    /// clip into memory so its length is known (what a player needs).
    #[default]
    Auto,
    /// Always decode on demand, even without `total_frames`. The frame count
    /// is then unknown (reported as 0) and rendering past the last frame
    /// returns [`Render::End`]. For thumbnails: only early frames are needed,
    /// and a long old-format clip would otherwise take ~200 MB to load.
    StreamOnly,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Render {
    Ok,
    /// No frame at that index (end of clip, or the file is truncated).
    End,
    /// Destination buffer too small.
    BadBuffer,
}

pub struct Clip {
    source: FrameSource,
    /// Number of playable frames; 0 if unknown (only with [`Load::StreamOnly`]).
    pub frame_count: usize,
    pub fps: f64,
    pub scale: usize,
    pub normalize: Normalize,
    /// Clip-wide min/max: from the header if present, otherwise scanned from
    /// the frames. `None` only when streaming with per-frame normalisation
    /// and the header doesn't have it (no point paying for a scan then).
    pub range: Option<(u16, u16)>,
    pub timestamp_us: u64,
    lut: [u32; 256],
    meta: Vec<CString>,
    /// Scratch buffer of normalised source values (reused per render).
    norm: Vec<f32>,
}

#[repr(C)]
pub struct CptvInfo {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub fps: f64,
    /// Recording start, microseconds since the Unix epoch (0 if unknown).
    pub timestamp_us: u64,
    /// 1 if frames are decoded on demand, 0 if the clip is held in memory.
    pub streaming: u32,
}

/// Metadata keys for `cptv_get_meta`.
pub const META_TITLE: c_int = 0;
pub const META_DATE: c_int = 1;
pub const META_DESCRIPTION: c_int = 2;
pub const META_DEVICE: c_int = 3;

fn is_gzip(buf: &[u8]) -> bool {
    buf.len() >= 2 && buf[0] == 0x1f && buf[1] == 0x8b
}

fn looks_like_cptv_header(buf: &[u8]) -> bool {
    buf.len() >= 6 && &buf[0..4] == b"CPTV" && matches!(buf[4], 1 | 2) && buf[5] == b'H'
}

/// Returns true if `buf` (the first few KB of a file) looks like a CPTV file,
/// either gzipped (as written by the cameras) or already decompressed.
pub fn probe(buf: &[u8]) -> bool {
    if looks_like_cptv_header(buf) {
        return true;
    }
    if !is_gzip(buf) {
        return false;
    }
    // Inflate just enough of the (possibly truncated) gzip stream to see the magic.
    let mut head = [0u8; 6];
    let mut dec = flate2::read::GzDecoder::new(buf);
    let mut got = 0;
    while got < head.len() {
        match dec.read(&mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => break,
        }
    }
    got == head.len() && looks_like_cptv_header(&head)
}

fn io_err(e: io::Error) -> String {
    e.to_string()
}

fn merge_range(a: Option<(u16, u16)>, px: &[u16]) -> Option<(u16, u16)> {
    let (lo, hi) = min_max(px);
    Some(match a {
        Some((alo, ahi)) => (alo.min(lo), ahi.max(hi)),
        None => (lo, hi),
    })
}

impl Clip {
    /// Open a clip held entirely in memory (gzipped or already inflated).
    pub fn open_bytes(
        data: Vec<u8>,
        palette: Palette,
        normalize: Normalize,
        scale: usize,
    ) -> Result<Clip, String> {
        Clip::open_bytes_with_load(data, palette, normalize, scale, Load::Auto)
    }

    pub fn open_bytes_with_load(
        data: Vec<u8>,
        palette: Palette,
        normalize: Normalize,
        scale: usize,
        load: Load,
    ) -> Result<Clip, String> {
        // The upstream decoder expects a gzip stream. Some VLC stream filters
        // may hand us already-inflated data, so re-wrap it if needed.
        let data = if is_gzip(&data) {
            data
        } else if looks_like_cptv_header(&data) {
            let mut enc = flate2::write::GzEncoder::new(
                Vec::with_capacity(data.len() / 2),
                flate2::Compression::fast(),
            );
            enc.write_all(&data).map_err(io_err)?;
            enc.finish().map_err(io_err)?
        } else {
            return Err("not a CPTV file".into());
        };
        let data: Arc<[u8]> = data.into();
        let open: Opener = Box::new(move || Ok(Box::new(Cursor::new(data.clone())) as _));
        Clip::open_with_load(open, palette, normalize, scale, load)
    }

    /// Open a clip from a re-openable byte stream. `open` is called once up
    /// front and again whenever a streaming clip has to rewind.
    pub fn open_with(
        open: Opener,
        palette: Palette,
        normalize: Normalize,
        scale: usize,
    ) -> Result<Clip, String> {
        Clip::open_with_load(open, palette, normalize, scale, Load::Auto)
    }

    pub fn open_with_load(
        mut open: Opener,
        palette: Palette,
        normalize: Normalize,
        scale: usize,
        load: Load,
    ) -> Result<Clip, String> {
        let mut magic = [0u8; 2];
        open().and_then(|mut r| r.read_exact(&mut magic)).map_err(io_err)?;
        if !is_gzip(&magic) {
            // Already-inflated input: buffer it and re-gzip (rare; only if a
            // stream filter decompressed it for us).
            let mut data = Vec::new();
            open().and_then(|mut r| r.read_to_end(&mut data)).map_err(io_err)?;
            return Clip::open_bytes_with_load(data, palette, normalize, scale, load);
        }

        let mut dec = new_decoder(&mut open).map_err(io_err)?;
        let header = dec.get_header().map_err(io_err)?;
        let header_range = match (header.min_value, header.max_value) {
            (Some(lo), Some(hi)) if hi > lo => Some((lo, hi)),
            _ => None,
        };

        let total = header.total_frame_count.filter(|&t| t > 0).map(usize::from);
        let (mut source, frame_count, range) = match (total, load) {
            (Some(_), _) | (None, Load::StreamOnly) => {
                let mut range = header_range;
                if range.is_none() && normalize == Normalize::Clip {
                    // One decode pass without keeping frames, then rewind.
                    while let Some(r) = next_playable(&mut dec, move |px| merge_range(range, px)) {
                        range = r;
                    }
                    dec = new_decoder(&mut open).map_err(io_err)?;
                    dec.get_header().map_err(io_err)?;
                }
                let src = FrameSource::Stream(StreamSource::new(open, dec));
                (src, total.unwrap_or(0), range)
            }
            (None, Load::Auto) => {
                let mut frames = Vec::new();
                let mut range = header_range;
                let scan = range.is_none();
                while let Some(px) = next_playable(&mut dec, |px| px.to_vec().into_boxed_slice()) {
                    if scan {
                        range = merge_range(range, &px);
                    }
                    frames.push(px);
                }
                let n = frames.len();
                (FrameSource::Memory(frames), n, range)
            }
        };
        // With an unknown count, decoding frame 0 (which stays cached) is the
        // cheap way to reject files that have no frames.
        let unknown_count = frame_count == 0 && load == Load::StreamOnly;
        if !(frame_count > 0 || unknown_count && matches!(source.frame(0), Fetch::Frame(_))) {
            return Err("CPTV file contains no frames".into());
        }

        let fps = if header.fps > 0 { header.fps as f64 } else { 9.0 };
        Ok(Clip {
            meta: build_meta(&header, range),
            source,
            frame_count,
            fps,
            scale: scale.clamp(1, 16),
            normalize,
            range,
            timestamp_us: header.timestamp,
            lut: palette.lut(),
            norm: vec![0.0; SRC_W * SRC_H],
        })
    }

    pub fn is_streaming(&self) -> bool {
        self.source.is_streaming()
    }

    pub fn width(&self) -> usize {
        SRC_W * self.scale
    }

    pub fn height(&self) -> usize {
        SRC_H * self.scale
    }

    /// Raw 160×120 sensor values of frame `index`.
    pub fn raw_frame(&mut self, index: usize) -> Option<&[u16]> {
        match self.source.frame(index) {
            Fetch::Frame(px) => Some(px),
            Fetch::End => None,
        }
    }

    /// Render frame `index` as native-endian 0xAARRGGBB pixels into `dst`,
    /// whose rows are `pitch` bytes apart.
    pub fn render(&mut self, index: usize, dst: &mut [u8], pitch: usize) -> Render {
        self.render_impl(index, false, dst, pitch)
    }

    /// Like [`render`](Self::render), but if the clip has fewer frames than
    /// `index + 1`, renders its last frame. Useful for thumbnails when the
    /// frame count is unknown ([`Load::StreamOnly`]).
    pub fn render_clamped(&mut self, index: usize, dst: &mut [u8], pitch: usize) -> Render {
        self.render_impl(index, true, dst, pitch)
    }

    fn render_impl(&mut self, index: usize, clamp: bool, dst: &mut [u8], pitch: usize) -> Render {
        let (out_w, out_h) = (self.width(), self.height());
        if pitch < out_w * 4 || dst.len() < pitch * (out_h - 1) + out_w * 4 {
            return Render::BadBuffer;
        }
        let fetched = if clamp {
            self.source.frame_clamped(index)
        } else {
            self.source.frame(index)
        };
        let pixels = match fetched {
            Fetch::Frame(px) => px,
            Fetch::End => return Render::End,
        };

        let (lo, hi) = match (self.normalize, self.range) {
            (Normalize::Clip, Some(r)) => r,
            _ => min_max(pixels),
        };
        let range = (hi.saturating_sub(lo)).max(1) as f32;
        for (n, &p) in self.norm.iter_mut().zip(pixels.iter()) {
            *n = (p.saturating_sub(lo) as f32 / range).clamp(0.0, 1.0);
        }

        let s = self.scale;
        let inv = 1.0 / s as f32;
        for oy in 0..out_h {
            let row = &mut dst[oy * pitch..oy * pitch + out_w * 4];
            // Bilinear sample position in source space (pixel centres aligned).
            let sy = ((oy as f32 + 0.5) * inv - 0.5).clamp(0.0, (SRC_H - 1) as f32);
            let y0 = sy as usize;
            let y1 = (y0 + 1).min(SRC_H - 1);
            let fy = sy - y0 as f32;
            let r0 = &self.norm[y0 * SRC_W..(y0 + 1) * SRC_W];
            let r1 = &self.norm[y1 * SRC_W..(y1 + 1) * SRC_W];
            for (ox, px) in row.chunks_exact_mut(4).enumerate() {
                let sx = ((ox as f32 + 0.5) * inv - 0.5).clamp(0.0, (SRC_W - 1) as f32);
                let x0 = sx as usize;
                let x1 = (x0 + 1).min(SRC_W - 1);
                let fx = sx - x0 as f32;
                let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
                let bot = r1[x0] + (r1[x1] - r1[x0]) * fx;
                let v = top + (bot - top) * fy;
                let c = self.lut[(v * 255.0 + 0.5) as usize & 0xff];
                px.copy_from_slice(&c.to_ne_bytes());
            }
        }
        Render::Ok
    }
}

fn build_meta(header: &CptvHeader, range: Option<(u16, u16)>) -> Vec<CString> {
    let device = header.device_name.as_string();
    let date = format_iso8601(header.timestamp);

    let mut desc = Vec::new();
    if !device.is_empty() {
        desc.push(format!("Device: {device}"));
    }
    if let Some(id) = header.device_id {
        desc.push(format!("Device ID: {id}"));
    }
    let camera: Vec<String> = [&header.brand, &header.model]
        .iter()
        .filter_map(|s| s.as_ref().map(|s| s.as_string()))
        .filter(|s| !s.is_empty())
        .collect();
    if !camera.is_empty() {
        desc.push(format!("Camera: {}", camera.join(" ")));
    }
    if let Some(serial) = header.serial_number {
        desc.push(format!("Serial: {serial}"));
    }
    if let Some(fw) = &header.firmware_version {
        desc.push(format!("Firmware: {}", fw.as_string()));
    }
    if let (Some(lat), Some(lon)) = (header.latitude, header.longitude) {
        desc.push(format!("Location: {lat:.5}, {lon:.5}"));
    }
    if let Some((lo, hi)) = range {
        desc.push(format!("Sensor range: {lo}–{hi}"));
    }

    let title = match (device.is_empty(), date.is_empty()) {
        (false, false) => format!("{device} – {date}"),
        (false, true) => device.clone(),
        (true, false) => date.clone(),
        (true, true) => String::new(),
    };

    [title, date, desc.join("\n"), device]
        .into_iter()
        .map(|s| CString::new(s.replace('\0', "")).unwrap_or_default())
        .collect()
}

fn min_max(px: &[u16]) -> (u16, u16) {
    px.iter()
        .fold((u16::MAX, u16::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)))
}

/// Format microseconds since the epoch as `YYYY-MM-DD HH:MM:SS UTC`.
fn format_iso8601(us: u64) -> String {
    if us == 0 {
        return String::new();
    }
    let secs = (us / 1_000_000) as i64;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        sod / 3600,
        (sod / 60) % 60,
        sod % 60
    )
}

// ---------------------------------------------------------------------------
// C ABI
// ---------------------------------------------------------------------------

/// Byte-stream callbacks supplied by the host (VLC's stream_t).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CptvIo {
    pub opaque: *mut c_void,
    /// Returns bytes read, 0 at end of stream, negative on error.
    pub read: Option<unsafe extern "C" fn(*mut c_void, *mut u8, usize) -> isize>,
    /// Seek to an absolute offset. Returns 0 on success.
    pub seek: Option<unsafe extern "C" fn(*mut c_void, u64) -> c_int>,
}

struct IoReader(CptvIo);

impl Read for IoReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.0.read.ok_or_else(|| io::Error::other("no read callback"))?;
        let n = unsafe { read(self.0.opaque, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Err(io::Error::other("stream read failed"))
        } else {
            Ok(n as usize)
        }
    }
}

fn io_opener(io: CptvIo) -> Opener {
    Box::new(move || {
        let seek = io.seek.ok_or_else(|| io::Error::other("no seek callback"))?;
        if unsafe { seek(io.opaque, 0) } != 0 {
            return Err(io::Error::other("cannot rewind stream"));
        }
        Ok(Box::new(IoReader(io)) as _)
    })
}

unsafe fn slice<'a>(buf: *const u8, len: usize) -> &'a [u8] {
    if buf.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(buf, len)
    }
}

unsafe fn write_err(err: *mut c_char, err_len: usize, msg: &str) {
    if err.is_null() || err_len == 0 {
        return;
    }
    let bytes = msg.as_bytes();
    let n = bytes.len().min(err_len - 1);
    ptr::copy_nonoverlapping(bytes.as_ptr(), err as *mut u8, n);
    *err.add(n) = 0;
}

/// Returns 1 if the buffer looks like the start of a CPTV file.
#[no_mangle]
pub unsafe extern "C" fn cptv_probe(buf: *const u8, len: usize) -> c_int {
    let data = slice(buf, len);
    catch_unwind(|| probe(data) as c_int).unwrap_or(0)
}

/// `cptv_open_io_flags` flag: decode on demand even without `total_frames`
/// in the header (see [`Load::StreamOnly`]).
pub const CPTV_OPEN_STREAM_ONLY: u32 = 1;

/// Open a CPTV clip read through `io`. The callbacks (and their `opaque`)
/// must stay valid until `cptv_close`. Returns NULL on failure, with a
/// message written to `err`.
#[no_mangle]
pub unsafe extern "C" fn cptv_open_io(
    io: *const CptvIo,
    palette: *const c_char,
    normalize_clip: c_int,
    scale: u32,
    err: *mut c_char,
    err_len: usize,
) -> *mut Clip {
    cptv_open_io_flags(io, palette, normalize_clip, scale, 0, err, err_len)
}

/// As `cptv_open_io`, with `CPTV_OPEN_*` flags.
#[no_mangle]
pub unsafe extern "C" fn cptv_open_io_flags(
    io: *const CptvIo,
    palette: *const c_char,
    normalize_clip: c_int,
    scale: u32,
    flags: u32,
    err: *mut c_char,
    err_len: usize,
) -> *mut Clip {
    let Some(&io) = io.as_ref() else {
        write_err(err, err_len, "no I/O callbacks");
        return ptr::null_mut();
    };
    let pal = if palette.is_null() {
        Palette::default()
    } else {
        Palette::from_name(&CStr::from_ptr(palette).to_string_lossy())
    };
    let norm = if normalize_clip != 0 { Normalize::Clip } else { Normalize::Frame };
    let load = if flags & CPTV_OPEN_STREAM_ONLY != 0 { Load::StreamOnly } else { Load::Auto };
    match catch_unwind(AssertUnwindSafe(|| {
        Clip::open_with_load(io_opener(io), pal, norm, scale as usize, load)
    })) {
        Ok(Ok(clip)) => Box::into_raw(Box::new(clip)),
        Ok(Err(e)) => {
            write_err(err, err_len, &e);
            ptr::null_mut()
        }
        Err(_) => {
            write_err(err, err_len, "decoder panicked");
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn cptv_get_info(clip: *const Clip, info: *mut CptvInfo) {
    let (Some(clip), Some(info)) = (clip.as_ref(), info.as_mut()) else {
        return;
    };
    *info = CptvInfo {
        width: clip.width() as u32,
        height: clip.height() as u32,
        frame_count: clip.frame_count as u32,
        fps: clip.fps,
        timestamp_us: clip.timestamp_us,
        streaming: clip.is_streaming() as u32,
    };
}

/// Returns a NUL-terminated string owned by the clip (valid until
/// `cptv_close`), or NULL if the value is unknown/empty.
#[no_mangle]
pub unsafe extern "C" fn cptv_get_meta(clip: *const Clip, key: c_int) -> *const c_char {
    let Some(clip) = clip.as_ref() else {
        return ptr::null();
    };
    match clip.meta.get(key as usize) {
        Some(s) if !s.as_bytes().is_empty() => s.as_ptr(),
        _ => ptr::null(),
    }
}

/// Render frame `index` into `dst` (RV32, `pitch` bytes per row, buffer
/// `dst_len` bytes). Returns 1 on success, 0 if there is no such frame (end
/// of clip), -1 on error.
#[no_mangle]
pub unsafe extern "C" fn cptv_render(
    clip: *mut Clip,
    index: u32,
    dst: *mut u8,
    dst_len: usize,
    pitch: usize,
) -> c_int {
    let Some(clip) = clip.as_mut() else { return -1 };
    if dst.is_null() {
        return -1;
    }
    let out = std::slice::from_raw_parts_mut(dst, dst_len);
    match catch_unwind(AssertUnwindSafe(|| clip.render(index as usize, out, pitch))) {
        Ok(Render::Ok) => 1,
        Ok(Render::End) => 0,
        _ => -1,
    }
}

/// As `cptv_render`, but renders the clip's last frame if it has fewer than
/// `index + 1` frames (returns 0 only if it has none).
#[no_mangle]
pub unsafe extern "C" fn cptv_render_clamped(
    clip: *mut Clip,
    index: u32,
    dst: *mut u8,
    dst_len: usize,
    pitch: usize,
) -> c_int {
    let Some(clip) = clip.as_mut() else { return -1 };
    if dst.is_null() {
        return -1;
    }
    let out = std::slice::from_raw_parts_mut(dst, dst_len);
    match catch_unwind(AssertUnwindSafe(|| clip.render_clamped(index as usize, out, pitch))) {
        Ok(Render::Ok) => 1,
        Ok(Render::End) => 0,
        _ => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn cptv_close(clip: *mut Clip) {
    if !clip.is_null() {
        drop(Box::from_raw(clip));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(format_iso8601(0), "");
        assert_eq!(format_iso8601(1_600_000_000_000_000), "2020-09-13 12:26:40 UTC");
    }

    #[test]
    fn probe_rejects_junk() {
        assert!(!probe(b"hello world"));
        assert!(!probe(&[0x1f, 0x8b]));
    }
}
