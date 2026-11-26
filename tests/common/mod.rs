//! Test helpers: locating cptv-tools' sample files, and a minimal CPTV v2
//! encoder (the upstream crate only decodes). Synthetic frames use 16-bit
//! big-endian deltas, the non-TC2 layout.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// cptv-tools' `cptv-codec-rs/tests/fixtures`, from Cargo's checkout of the
/// `codec` dependency at the commit pinned in Cargo.toml.
pub fn fixtures_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .output()
            .expect("failed to run cargo metadata");
        let json = String::from_utf8_lossy(&out.stdout);
        let suffix = "/cptv-codec-rs/Cargo.toml";
        let end = json.find(suffix).expect("cptv-tools checkout not found in cargo metadata")
            + suffix.len();
        let key = "\"manifest_path\":\"";
        let start = json[..end].rfind(key).unwrap() + key.len();
        let manifest = PathBuf::from(&json[start..end]);
        let dir = manifest.parent().unwrap().join("tests/fixtures");
        assert!(dir.is_dir(), "no fixtures at {}", dir.display());
        dir
    })
}

pub fn fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub const W: usize = 160;
pub const H: usize = 120;

pub struct Synth {
    pub frames: usize,
    pub fps: u8,
    /// Value written to the header's total_frames field, if any.
    pub total_in_header: Option<u16>,
    pub minmax_in_header: Option<(u16, u16)>,
}

/// Pixel 0 of frame `f` encodes the frame index, so tests can check which
/// frame they got back.
pub fn marker(f: usize) -> u16 {
    5000 + (f % 3000) as u16
}

pub fn pixel(f: usize, p: usize) -> u16 {
    if p == 0 {
        marker(f)
    } else {
        3000 + ((p * 7 + f * 13) % 200) as u16
    }
}

fn field(out: &mut Vec<u8>, code: u8, val: &[u8]) {
    out.push(val.len() as u8);
    out.push(code);
    out.extend_from_slice(val);
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// Snake scan order used by the codec.
fn snake() -> Vec<usize> {
    (0..H)
        .flat_map(|y| {
            let row: Vec<usize> = if y % 2 == 0 { (0..W).collect() } else { (0..W).rev().collect() };
            row.into_iter().map(move |x| y * W + x)
        })
        .collect()
}

impl Synth {
    pub fn encode(&self) -> Vec<u8> {
        let mut header = b"CPTV\x02H".to_vec();
        let mut fields = Vec::new();
        field(&mut fields, b'X', &(W as u32).to_le_bytes());
        field(&mut fields, b'Y', &(H as u32).to_le_bytes());
        field(&mut fields, b'C', &[1]);
        field(&mut fields, b'T', &1_700_000_000_000_000u64.to_le_bytes());
        field(&mut fields, b'D', b"synthetic");
        field(&mut fields, b'Z', &[self.fps]);
        field(&mut fields, b'V', b"test");
        let mut n = 7;
        if let Some(t) = self.total_in_header {
            field(&mut fields, b'J', &t.to_le_bytes());
            n += 1;
        }
        if let Some((lo, hi)) = self.minmax_in_header {
            field(&mut fields, b'Q', &lo.to_le_bytes());
            field(&mut fields, b'K', &hi.to_le_bytes());
            n += 2;
        }
        header.push(n);
        header.extend_from_slice(&fields);

        let order = snake();
        let data_len = 4 + (W * H - 1) * 2;
        let mut body = Vec::with_capacity(self.frames * (data_len + 20));
        let mut prev = vec![0u16; W * H];
        for f in 0..self.frames {
            let cur: Vec<u16> = (0..W * H).map(|p| pixel(f, p)).collect();
            body.push(b'F');
            body.push(3);
            field(&mut body, b't', &((f as u32) * 111).to_le_bytes());
            field(&mut body, b'w', &[16]);
            field(&mut body, b'f', &(data_len as u32).to_le_bytes());
            let d = |i: usize| cur[i] as i32 - prev[i] as i32;
            body.extend_from_slice(&d(order[0]).to_le_bytes());
            for k in 1..order.len() {
                let delta = (d(order[k]) - d(order[k - 1])) as i16;
                body.extend_from_slice(&delta.to_be_bytes());
            }
            prev = cur;
        }
        // Header and frames are separate gzip members, like the cameras write.
        let mut out = gzip(&header);
        out.extend(gzip(&body));
        out
    }
}
