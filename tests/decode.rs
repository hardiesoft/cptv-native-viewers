mod common;

use std::time::Instant;

use common::{fixture, marker, Synth};
use cptv_vlc::palette::Palette;
use cptv_vlc::{probe, Clip, Normalize, Render};


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
