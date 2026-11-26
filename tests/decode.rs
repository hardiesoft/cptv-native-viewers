mod common;

use std::time::Instant;

use common::{fixture, marker, Synth};
use cptv_native_viewers::palette::Palette;
use cptv_native_viewers::{probe, Clip, Load, Normalize, Render};


fn open(data: Vec<u8>, norm: Normalize) -> Clip {
    Clip::open_bytes(data, Palette::Viridis, norm, 1).unwrap()
}

#[test]
fn probes_gzipped_and_raw() {
    let data = fixture("20240507-074536.cptv");
    assert!(probe(&data[..4096]));
    let mut raw = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::MultiGzDecoder::new(&data[..]), &mut raw).unwrap();
    assert!(probe(&raw[..64]));
    // Already-inflated input decodes identically.
    let a = open(data, Normalize::Frame);
    let b = open(raw, Normalize::Frame);
    assert_eq!(a.frame_count, b.frame_count);
}

#[test]
fn decodes_v1_clip_into_memory() {
    let clip = Clip::open_bytes(fixture("20180603-090916.cptv"), Palette::Grey, Normalize::Clip, 2).unwrap();
    assert!(!clip.is_streaming(), "no total_frames in header");
    assert_eq!(clip.frame_count, 310);
    assert_eq!((clip.width(), clip.height()), (320, 240));
}

#[test]
fn opens_every_upstream_fixture() {
    // (file, playable frames, streamed because the header has total_frames)
    let expected = [
        ("20180603-090916.cptv", 310, false),
        ("20201221-748923.cptv", 379, false),
        ("20240507-074536.cptv", 46, true),
        ("20240917-1921337.cptv", 105, false), // + 1 background frame
    ];
    let mut names: Vec<_> = std::fs::read_dir(common::fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".cptv"))
        .collect();
    names.sort();
    for name in &names {
        let mut clip = open(fixture(name), Normalize::Clip);
        let mut buf = vec![0u8; clip.width() * clip.height() * 4];
        let pitch = clip.width() * 4;
        let last = clip.frame_count - 1;
        assert_eq!(clip.render(last, &mut buf, pitch), Render::Ok, "{name}");
        assert_eq!(clip.render(0, &mut buf, pitch), Render::Ok, "{name}");
        if let Some(&(_, frames, streaming)) = expected.iter().find(|e| e.0 == name) {
            assert_eq!((clip.frame_count, clip.is_streaming()), (frames, streaming), "{name}");
        } else {
            println!("new upstream fixture {name}: {} frames", clip.frame_count);
        }
    }
    assert!(names.len() >= expected.len(), "expected fixtures missing: {names:?}");
}

#[test]
fn renders_every_frame() {
    let mut clip = Clip::open_bytes(fixture("20240507-074536.cptv"), Palette::Ironbow, Normalize::Frame, 4).unwrap();
    let (w, h) = (clip.width(), clip.height());
    let mut buf = vec![0u8; w * h * 4];
    for i in 0..clip.frame_count {
        assert_eq!(clip.render(i, &mut buf, w * 4), Render::Ok);
    }
    assert_eq!(clip.render(clip.frame_count, &mut buf, w * 4), Render::End);
    // Image shouldn't be a single flat colour.
    let px: std::collections::HashSet<_> = buf.chunks_exact(4).collect();
    assert!(px.len() > 50);
}

#[test]
fn clip_range_from_header_when_present() {
    // This file records total_frames and min/max in its header.
    let clip = open(fixture("20240507-074536.cptv"), Normalize::Clip);
    assert!(clip.is_streaming());
    assert_eq!(clip.frame_count, 46);
    assert_eq!(clip.range, Some((27434, 27789)));
}

#[test]
fn clip_range_scanned_when_header_lacks_it() {
    // This (v1) file has no min/max header fields, so they come from the frames.
    let clip = open(fixture("20180603-090916.cptv"), Normalize::Clip);
    assert_eq!(clip.range, Some((2917, 3321)));
}

#[test]
fn streamed_and_in_memory_frames_match() {
    // Streamed frames (this file's header has total_frames) must match a
    // plain sequential decode with the upstream decoder.
    let data = fixture("20240507-074536.cptv");
    let mut streamed = open(data.clone(), Normalize::Clip);
    let mut reference = Vec::new();
    let mut dec = codec::decode::CptvDecoder::<std::io::Cursor<Vec<u8>>>::from_bytes(data).unwrap();
    while let Ok(f) = dec.next_frame() {
        if !f.is_background_frame {
            reference.push(f.image_data.data().to_vec());
        }
    }
    // Out-of-order access exercises forward skips and rewinds.
    for &i in &[0, 1, 2, 30, 5, 45, 44, 0, 45] {
        assert_eq!(streamed.raw_frame(i).unwrap(), &reference[i][..], "frame {i}");
    }
}

#[test]
fn ten_minute_clip_streams_and_seeks() {
    let frames = 9 * 60 * 10; // 10 minutes at 9 fps
    let data = Synth { frames, fps: 9, total_in_header: Some(frames as u16), minmax_in_header: Some((3000, 8000)) }.encode();
    let t = Instant::now();
    let mut clip = open(data, Normalize::Clip);
    let open_time = t.elapsed();
    assert!(clip.is_streaming());
    assert_eq!(clip.frame_count, frames);
    assert_eq!(clip.range, Some((3000, 8000)));

    let check = |clip: &mut Clip, i: usize| {
        let t = Instant::now();
        assert_eq!(clip.raw_frame(i).unwrap()[0], marker(i), "frame {i}");
        t.elapsed()
    };
    for i in 0..20 {
        check(&mut clip, i);
    }
    let fwd = check(&mut clip, 4000);
    let back = check(&mut clip, frames - 2); // forward again
    let rewind = check(&mut clip, 10); // backward: rewinds
    let worst = check(&mut clip, frames - 1); // full decode from the start
    assert!(clip.raw_frame(frames).is_none());
    println!("open {open_time:?}, seek fwd {fwd:?}, to end {back:?}, rewind {rewind:?}, rewind+to end {worst:?}");
}

#[test]
fn streaming_scans_range_when_header_lacks_minmax() {
    let frames = 50;
    let data = Synth { frames, fps: 9, total_in_header: Some(frames as u16), minmax_in_header: None }.encode();
    let mut clip = open(data.clone(), Normalize::Clip);
    assert!(clip.is_streaming());
    assert_eq!(clip.range, Some((3000, marker(frames - 1))));
    // ...and is still positioned at the start afterwards.
    assert_eq!(clip.raw_frame(0).unwrap()[0], marker(0));
    // With per-frame normalisation no scan is needed.
    assert_eq!(open(data, Normalize::Frame).range, None);
}

#[test]
fn no_total_frames_decodes_into_memory() {
    let data = Synth { frames: 30, fps: 9, total_in_header: None, minmax_in_header: None }.encode();
    let mut clip = open(data, Normalize::Clip);
    assert!(!clip.is_streaming());
    assert_eq!(clip.frame_count, 30);
    assert_eq!(clip.raw_frame(29).unwrap()[0], marker(29));
}

#[test]
fn header_overstating_frame_count_ends_cleanly() {
    // e.g. a truncated recording whose header claims more frames than exist.
    let data = Synth { frames: 20, fps: 9, total_in_header: Some(100), minmax_in_header: Some((3000, 8000)) }.encode();
    let mut clip = open(data, Normalize::Clip);
    let mut buf = vec![0u8; clip.width() * clip.height() * 4];
    let pitch = clip.width() * 4;
    assert_eq!(clip.render(19, &mut buf, pitch), Render::Ok);
    assert_eq!(clip.render(20, &mut buf, pitch), Render::End);
    // Seeking back after hitting the end still works.
    assert_eq!(clip.render(3, &mut buf, pitch), Render::Ok);
}

/// Writes synthetic 10-minute clips for manual testing in VLC:
/// `CPTV_SYNTH_DIR=/some/dir cargo test --release -- --ignored write_synthetic`
#[test]
#[ignore]
fn write_synthetic_clips() {
    let dir = std::env::var("CPTV_SYNTH_DIR").expect("set CPTV_SYNTH_DIR");
    let frames = 9 * 60 * 10;
    let with_total = Synth { frames, fps: 9, total_in_header: Some(frames as u16), minmax_in_header: Some((3000, 8000)) };
    let without = Synth { total_in_header: None, ..with_total };
    std::fs::write(format!("{dir}/synthetic-10min-streaming.cptv"), with_total.encode()).unwrap();
    std::fs::write(format!("{dir}/synthetic-10min-in-memory.cptv"), without.encode()).unwrap();
}

fn open_stream_only(data: Vec<u8>) -> Result<Clip, String> {
    Clip::open_bytes_with_load(data, Palette::Viridis, Normalize::Frame, 1, Load::StreamOnly)
}

#[test]
fn stream_only_never_loads_old_clips_into_memory() {
    // No total_frames in the header: Auto would decode all of it up front.
    let frames = 9 * 60 * 10;
    let data = Synth { frames, fps: 9, total_in_header: None, minmax_in_header: None }.encode();
    let mut clip = open_stream_only(data).unwrap();
    assert!(clip.is_streaming());
    assert_eq!(clip.frame_count, 0, "count is unknown without decoding everything");
    assert_eq!(clip.raw_frame(9).unwrap()[0], marker(9));
    assert_eq!(clip.raw_frame(0).unwrap()[0], marker(0)); // rewind
    assert_eq!(clip.raw_frame(frames - 1).unwrap()[0], marker(frames - 1));
    assert!(clip.raw_frame(frames).is_none(), "past the end");
}

#[test]
fn stream_only_keeps_header_frame_count() {
    let clip = open_stream_only(fixture("20240507-074536.cptv")).unwrap();
    assert_eq!((clip.frame_count, clip.is_streaming()), (46, true));
}

#[test]
fn stream_only_short_and_empty_clips() {
    // Shorter than the thumbnail's one-second target: frame 0 is still there.
    let short = Synth { frames: 3, fps: 9, total_in_header: None, minmax_in_header: None }.encode();
    let mut clip = open_stream_only(short).unwrap();
    let mut buf = vec![0u8; clip.width() * clip.height() * 4];
    let pitch = clip.width() * 4;
    assert_eq!(clip.render(9, &mut buf, pitch), Render::End);
    assert_eq!(clip.render(0, &mut buf, pitch), Render::Ok);
    // Header but no frames is still an error, as with Load::Auto.
    let empty = Synth { frames: 0, fps: 9, total_in_header: None, minmax_in_header: None }.encode();
    assert!(open_stream_only(empty).is_err());
}

#[test]
fn stream_only_matches_auto_on_every_upstream_fixture() {
    for name in ["20180603-090916.cptv", "20201221-748923.cptv", "20240507-074536.cptv", "20240917-1921337.cptv"] {
        let mut auto = open(fixture(name), Normalize::Frame);
        let mut lazy = open_stream_only(fixture(name)).unwrap();
        for i in [9, 0, auto.frame_count - 1] {
            assert_eq!(auto.raw_frame(i).unwrap().to_vec(), lazy.raw_frame(i).unwrap(), "{name} frame {i}");
        }
    }
}

/// Render `index` (clamped or not) and return the RGB bytes.
fn rendered(clip: &mut Clip, index: usize, clamped: bool) -> Option<Vec<u8>> {
    let pitch = clip.width() * 4;
    let mut buf = vec![0u8; pitch * clip.height()];
    let r = if clamped { clip.render_clamped(index, &mut buf, pitch) } else { clip.render(index, &mut buf, pitch) };
    (r == Render::Ok).then_some(buf)
}

#[test]
fn render_clamped_gives_last_frame_of_short_clips() {
    // Like cacophony-web's oneframe.cptv: no total_frames, under a second long.
    for (total, load) in [(None, Load::StreamOnly), (None, Load::Auto), (Some(4), Load::StreamOnly)] {
        let data = Synth { frames: 4, fps: 9, total_in_header: total, minmax_in_header: None }.encode();
        let mut clip = Clip::open_bytes_with_load(data, Palette::Grey, Normalize::Frame, 1, load).unwrap();
        let last = rendered(&mut clip, 3, false).unwrap();
        assert_eq!(rendered(&mut clip, 9, true).unwrap(), last, "{total:?} {load:?}");
        // Also after a plain render has already run off the end.
        assert!(rendered(&mut clip, 9, false).is_none());
        assert_eq!(rendered(&mut clip, 9, true).unwrap(), last, "{total:?} {load:?} after End");
        // In range, clamped is the same as plain.
        let first = rendered(&mut clip, 0, false).unwrap();
        assert_eq!(rendered(&mut clip, 0, true).unwrap(), first);
    }
}
