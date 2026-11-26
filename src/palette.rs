//! Colour maps used to turn normalised thermal values into RGB.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Palette {
    #[default]
    Viridis,
    Inferno,
    Magma,
    Plasma,
    Turbo,
    Ironbow,
    Grey,
}

/// Names accepted by the `--cptv-palette` option (first entry is the default).
pub const NAMES: &[&str] = &["viridis", "inferno", "magma", "plasma", "turbo", "ironbow", "grey"];

// Classic "ironbow" thermal-camera palette, as (position, r, g, b) stops.
const IRONBOW: &[(f32, [u8; 3])] = &[
    (0.00, [0, 0, 10]),
    (0.15, [32, 0, 100]),
    (0.30, [120, 0, 150]),
    (0.45, [190, 30, 110]),
    (0.60, [230, 80, 30]),
    (0.75, [250, 150, 0]),
    (0.90, [255, 220, 60]),
    (1.00, [255, 255, 240]),
];

impl Palette {
    pub fn from_name(name: &str) -> Palette {
        match name.trim().to_ascii_lowercase().as_str() {
            "inferno" => Palette::Inferno,
            "magma" => Palette::Magma,
            "plasma" => Palette::Plasma,
            "turbo" => Palette::Turbo,
            "ironbow" | "iron" => Palette::Ironbow,
            "grey" | "gray" | "greyscale" | "grayscale" | "white-hot" => Palette::Grey,
            _ => Palette::Viridis,
        }
    }

    fn rgb(self, t: f32) -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        let g = |grad: colorous::Gradient| {
            let c = grad.eval_continuous(t as f64);
            [c.r, c.g, c.b]
        };
        match self {
            Palette::Viridis => g(colorous::VIRIDIS),
            Palette::Inferno => g(colorous::INFERNO),
            Palette::Magma => g(colorous::MAGMA),
            Palette::Plasma => g(colorous::PLASMA),
            Palette::Turbo => g(colorous::TURBO),
            Palette::Grey => {
                let v = (t * 255.0 + 0.5) as u8;
                [v, v, v]
            }
            Palette::Ironbow => {
                let i = IRONBOW.windows(2).position(|w| t <= w[1].0).unwrap_or(IRONBOW.len() - 2);
                let ((p0, c0), (p1, c1)) = (IRONBOW[i], IRONBOW[i + 1]);
                let f = (t - p0) / (p1 - p0);
                let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f + 0.5) as u8;
                [lerp(c0[0], c1[0]), lerp(c0[1], c1[1]), lerp(c0[2], c1[2])]
            }
        }
    }

    /// 256-entry lookup table of native-endian 0xAARRGGBB pixels.
    pub fn lut(self) -> [u32; 256] {
        let mut lut = [0u32; 256];
        for (i, e) in lut.iter_mut().enumerate() {
            let [r, g, b] = self.rgb(i as f32 / 255.0);
            *e = 0xff00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
        lut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for n in NAMES {
            let p = Palette::from_name(n);
            assert!(n.starts_with(&format!("{p:?}").to_ascii_lowercase()[..3]));
        }
    }

    #[test]
    fn grey_endpoints() {
        let lut = Palette::Grey.lut();
        assert_eq!(lut[0], 0xff000000);
        assert_eq!(lut[255], 0xffffffff);
    }
}
