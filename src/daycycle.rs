#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plate {
    Night = 0,
    Dawn = 1,
    Day = 2,
    Dusk = 3,
}

pub const PLATE_NAMES: [&str; 4] = ["night", "dawn", "day", "dusk"];

#[derive(Clone, Copy, Debug)]
pub struct Keyframe {
    pub minute: u32,
    pub plate: Plate,
    pub tint: [f32; 3],
}

const NIGHT_TINT: [f32; 3] = [0.52, 0.54, 0.85];
const DAWN_TINT: [f32; 3] = [1.00, 0.97, 1.00];
const DAY_TINT: [f32; 3] = [1.00, 1.00, 1.00];
const DUSK_TINT: [f32; 3] = [0.99, 0.72, 0.76];

const fn kf(h: u32, m: u32, plate: Plate, tint: [f32; 3]) -> Keyframe {
    Keyframe { minute: h * 60 + m, plate, tint }
}

pub const DEFAULT_KEYFRAMES: [Keyframe; 6] = [
    kf(5, 0, Plate::Night, NIGHT_TINT),
    kf(6, 30, Plate::Dawn, DAWN_TINT),
    kf(9, 30, Plate::Day, DAY_TINT),
    kf(16, 30, Plate::Day, DAY_TINT),
    kf(18, 0, Plate::Dusk, DUSK_TINT),
    kf(19, 30, Plate::Night, NIGHT_TINT),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub a: Plate,
    pub b: Plate,
    pub t: f32,
    pub tint: [f32; 3],
}

pub const DAY_MINUTES: f32 = 24.0 * 60.0;

pub fn mix(kfs: &[Keyframe], minute: f32) -> Mix {
    let m = minute.rem_euclid(DAY_MINUTES);
    let n = kfs.len();
    let i = kfs.iter().rposition(|k| k.minute as f32 <= m).unwrap_or(n - 1);
    let (k0, k1) = (kfs[i], kfs[(i + 1) % n]);
    let start = k0.minute as f32;
    let mut span = k1.minute as f32 - start;
    let mut pos = m - start;
    if span <= 0.0 {
        span += DAY_MINUTES; // wraps past midnight
    }
    if pos < 0.0 {
        pos += DAY_MINUTES;
    }
    let t = (pos / span).clamp(0.0, 1.0);
    let tint = std::array::from_fn(|c| k0.tint[c] + (k1.tint[c] - k0.tint[c]) * t);
    Mix { a: k0.plate, b: k1.plate, t, tint }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> Mix {
        mix(&DEFAULT_KEYFRAMES, (h * 60 + m) as f32)
    }

    #[test]
    fn keyframes_and_blends() {
        assert_eq!((at(12, 0).a, at(12, 0).b), (Plate::Day, Plate::Day));
        let m = at(5, 45); // halfway 05:00 night -> 06:30 dawn
        assert_eq!((m.a, m.b), (Plate::Night, Plate::Dawn));
        assert!((m.t - 0.5).abs() < 1e-6);
        let m = at(18, 45); // halfway 18:00 dusk -> 19:30 night
        assert_eq!((m.a, m.b), (Plate::Dusk, Plate::Night));
        assert!((m.t - 0.5).abs() < 1e-6);
        for (h, mm) in [(0, 0), (2, 0), (4, 59), (19, 30), (23, 59)] {
            let m = at(h, mm);
            assert_eq!((m.a, m.b), (Plate::Night, Plate::Night), "{h}:{mm}");
        }
        assert_eq!(at(9, 30).a, Plate::Day);
        assert_eq!(at(9, 30).tint, DAY_TINT);
    }
}
