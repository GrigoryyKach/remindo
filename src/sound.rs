//! The built-in chime: a soft two-note bell made in code (no sound files), played when a timer fires.

use std::sync::OnceLock;

const RATE: u32 = 44_100;

/// A 16-bit mono WAV: two bell notes (A5, then E6) with a short attack and a long decay.
pub fn chime_wav() -> Vec<u8> {
    let seconds = 1.6f32;
    let n = (RATE as f32 * seconds) as usize;
    let mut pcm = vec![0f32; n];
    for (start, freq, gain) in [(0.0f32, 880.0f32, 0.30f32), (0.22, 1318.5, 0.26)] {
        let s0 = (start * RATE as f32) as usize;
        for (i, out) in pcm.iter_mut().enumerate().skip(s0) {
            let t = (i - s0) as f32 / RATE as f32;
            let attack = (t / 0.008).min(1.0);
            let decay = (-t * 3.2).exp();
            // the note, a soft second harmonic and a quiet inharmonic partial give the bell its colour
            let tone = (2.0 * std::f32::consts::PI * freq * t).sin() + 0.25 * (2.0 * std::f32::consts::PI * freq * 2.0 * t).sin() + 0.08 * (2.0 * std::f32::consts::PI * freq * 2.76 * t).sin();
            *out += gain * attack * decay * tone;
        }
    }
    // Fade the very end so there is no click.
    let fade = (RATE as f32 * 0.05) as usize;
    for k in 0..fade {
        pcm[n - 1 - k] *= k as f32 / fade as f32;
    }
    let mut data = Vec::with_capacity(n * 2);
    for v in pcm {
        data.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32_000.0) as i16).to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

static WAV: OnceLock<Vec<u8>> = OnceLock::new();

fn wav() -> &'static [u8] {
    WAV.get_or_init(chime_wav)
}

/// Plays the chime without blocking. A test instance (own data folder) stays silent.
pub fn play() {
    if crate::config::is_test_instance() {
        return;
    }
    play_now();
}

/// Plays it even in a test instance (the "Play" button in Settings).
#[cfg(windows)]
pub fn play_now() {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    // SAFETY: the buffer is a static that lives as long as the process, as SND_ASYNC requires.
    unsafe {
        PlaySoundW(wav().as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

#[cfg(not(windows))]
pub fn play_now() {
    use std::process::{Command, Stdio};
    let path = std::env::temp_dir().join(format!("remindo-chime-{}.wav", unsafe { libc::getuid() }));
    if !path.exists() {
        let _ = std::fs::write(&path, wav());
    }
    // The first player that exists wins: PulseAudio, PipeWire, ALSA.
    for (cmd, args) in [("paplay", vec![]), ("pw-play", vec![]), ("aplay", vec!["-q"])] {
        if Command::new(cmd).args(&args).arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chime_is_a_valid_quiet_wav() {
        let w = chime_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(w.len(), 44 + data_len);
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, w.len() - 8);
        let samples: Vec<i16> = w[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        // audible but gentle: well below full scale
        assert!(peak > 3_000 && peak < 20_000, "peak {peak}");
        // it ends in silence
        assert!(samples[samples.len() - 1].unsigned_abs() < 100);
        // about a second and a half
        assert!((1.4..1.8).contains(&(samples.len() as f32 / RATE as f32)));
    }
}
