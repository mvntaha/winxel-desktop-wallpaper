use std::sync::OnceLock;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

const RATE: u32 = 22_050;
const VOLUME: f32 = 0.16;

static WAV: OnceLock<Vec<u8>> = OnceLock::new();

pub fn play() {
    let wav = WAV.get_or_init(synth);
    unsafe {
        let _ = PlaySoundW(PCWSTR(wav.as_ptr().cast()), HMODULE::default(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

fn synth() -> Vec<u8> {
    let len = (RATE as f32 * 1.2) as usize;
    let notes = [(659.25f32, 0.0f32), (880.0, 0.22)];
    let samples: Vec<i16> = (0..len)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let mut v = 0.0;
            for (freq, start) in notes {
                let dt = t - start;
                if dt >= 0.0 {
                    let env = (dt / 0.01).min(1.0) * (-dt / 0.28).exp();
                    let w = (dt * freq * std::f32::consts::TAU).sin() + 0.25 * (dt * freq * 2.0 * std::f32::consts::TAU).sin();
                    v += w * env;
                }
            }
            (v * VOLUME * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16
        })
        .collect();
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_header_is_consistent() {
        let w = super::synth();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, w.len() - 8);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize, w.len() - 44);
    }
}
