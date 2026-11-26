//! Where decoded frames come from.
//!
//! * [`FrameSource::Memory`]: every playable frame is decoded once and kept as
//!   raw 16-bit pixels, so random access is free. Used when the header doesn't
//!   say how many frames there are, because then the clip length (and so the
//!   seek bar) is only known after decoding all of it.
//! * [`FrameSource::Stream`]: used when the header has `total_frames`. Frames
//!   are decoded on demand from the underlying byte stream and only the
//!   current one is kept, so a 10-minute clip doesn't need ~200 MB of RAM.
//!   CPTV frames are delta-coded against the previous frame inside a single
//!   gzip stream, so there is nowhere to jump into the middle: seeking
//!   forward decodes (and discards) up to the target, and seeking backward
//!   rewinds the input and decodes from the start (~70 µs per frame).

use std::io::{self, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};

use codec::decode::CptvDecoder;

pub type Reader = Box<dyn Read>;
pub type Decoder = CptvDecoder<Reader>;

/// Opens the input from its first byte. Called again for each rewind.
pub type Opener = Box<dyn FnMut() -> io::Result<Reader>>;

pub fn new_decoder(open: &mut Opener) -> io::Result<Decoder> {
    Ok(CptvDecoder::new_with_read(open()?))
}

/// Outcome of asking a source for a frame.
pub enum Fetch<'a> {
    Frame(&'a [u16]),
    /// The index is past the last frame that could be decoded.
    End,
}

pub enum FrameSource {
    Memory(Vec<Box<[u16]>>),
    Stream(StreamSource),
}

impl FrameSource {
    pub fn frame(&mut self, index: usize) -> Fetch<'_> {
        match self {
            FrameSource::Memory(frames) => match frames.get(index) {
                Some(f) => Fetch::Frame(f),
                None => Fetch::End,
            },
            FrameSource::Stream(s) => s.frame(index),
        }
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self, FrameSource::Stream(_))
    }
}

/// Decode the next non-background frame, calling `f` with its pixels.
/// Returns `None` at the end of the input or on a decode error/panic (the
/// decoder can assert on truncated or corrupt data).
pub fn next_playable<T>(dec: &mut Decoder, mut f: impl FnMut(&[u16]) -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(|| loop {
        let frame = dec.next_frame().ok()?;
        if !frame.is_background_frame {
            return Some(f(frame.image_data.data()));
        }
    }))
    .ok()
    .flatten()
}

pub struct StreamSource {
    open: Opener,
    /// `None` once the input is exhausted or broken, until the next rewind.
    decoder: Option<Decoder>,
    /// Index of the frame the decoder will produce next.
    next: usize,
    /// The most recently fetched frame and its index.
    current: Option<usize>,
    pixels: Box<[u16]>,
}

impl StreamSource {
    /// `decoder` must be positioned at the first frame (header already read).
    pub fn new(open: Opener, decoder: Decoder) -> StreamSource {
        StreamSource {
            open,
            decoder: Some(decoder),
            next: 0,
            current: None,
            pixels: vec![0; codec::common::WIDTH * codec::common::HEIGHT].into_boxed_slice(),
        }
    }

    fn rewind(&mut self) {
        self.next = 0;
        self.current = None;
        self.decoder = new_decoder(&mut self.open).ok();
        if let Some(dec) = self.decoder.as_mut() {
            if dec.get_header().is_err() {
                self.decoder = None;
            }
        }
    }

    fn frame(&mut self, index: usize) -> Fetch<'_> {
        if self.current == Some(index) {
            return Fetch::Frame(&self.pixels);
        }
        if index < self.next {
            self.rewind();
        }
        while self.next <= index {
            let Some(dec) = self.decoder.as_mut() else {
                return Fetch::End;
            };
            let want = self.next == index;
            let pixels = &mut self.pixels;
            // Only copy the pixels out for the frame actually asked for.
            let ok = next_playable(dec, |px| {
                if want {
                    pixels.copy_from_slice(px);
                }
            });
            if ok.is_none() {
                self.decoder = None;
                self.current = None;
                return Fetch::End;
            }
            self.next += 1;
        }
        self.current = Some(index);
        Fetch::Frame(&self.pixels)
    }
}
