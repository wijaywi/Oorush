//! Deterministic, dependency-free piano synthesis for Oorush artifacts.
//!
//! The public key (or any other canonical crypto seed) is used as input to a
//! domain-separated SHA-256 stream.  The resulting melody is therefore
//! reproducible without making the WAV file itself part of the seed.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::f64::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;
const DOMAIN: &[u8] = b"OORUSH_PIANO_WAV_V1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PianoMetadata {
    pub seed_hash: String,
    pub note_count: usize,
    pub tempo_bpm: u32,
    pub duration_ms: u64,
    pub audio_sha256: String,
}

/// Render a reproducible solo-piano WAV file from a crypto-derived seed.
///
/// The melody is intentionally modest: a C-major, single-note line in 4/4.
/// No external soundfont, model, GPU, or paid API is required.
pub fn generate_from_seed(seed: &[u8], output_path: impl AsRef<Path>) -> Result<PianoMetadata> {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(seed);
    let digest = hasher.finalize();
    let seed_hash = hex::encode(digest);

    let note_count = 8 + (digest[0] % 9) as usize;
    let tempo_bpm = 72 + (digest[1] % 49) as u32;
    let beat_seconds = 60.0 / tempo_bpm as f64;
    let beat_samples = (beat_seconds * SAMPLE_RATE as f64).round() as usize;
    let total_samples = note_count * beat_samples;
    let scale = [48_u8, 50, 52, 53, 55, 57, 59, 60, 62, 64, 65, 67];
    let mut pcm = Vec::with_capacity(total_samples * 2);

    for sample in 0..total_samples {
        let note_index = sample / beat_samples;
        let local_sample = sample % beat_samples;
        let note = scale[(digest[2 + (note_index % 30)] as usize) % scale.len()];
        let frequency = 440.0 * 2.0_f64.powf((note as f64 - 69.0) / 12.0);
        let t = local_sample as f64 / SAMPLE_RATE as f64;
        let note_length = beat_samples as f64 / SAMPLE_RATE as f64;
        let attack = (t / 0.012).min(1.0);
        let release = ((note_length - t) / 0.16).clamp(0.0, 1.0);
        let envelope = attack * release * (-3.2 * t).exp();
        let phase = 2.0 * PI * frequency * t;
        let tone = phase.sin()
            + 0.42 * (2.0 * phase).sin()
            + 0.19 * (3.0 * phase).sin()
            + 0.08 * (4.0 * phase).sin();
        let velocity = 0.72 + (digest[2 + (note_index % 30)] as f64 / 255.0) * 0.2;
        let value = (tone * envelope * velocity * 0.42 * i16::MAX as f64)
            .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }

    let output_path = output_path.as_ref();
    let file = File::create(output_path)
        .with_context(|| format!("cannot create piano output {}", output_path.display()))?;
    let mut writer = BufWriter::new(file);
    write_wav_header(&mut writer, pcm.len() as u32)?;
    writer.write_all(&pcm)?;
    writer.flush()?;

    let audio_sha256 = hex::encode(Sha256::digest(&pcm));
    Ok(PianoMetadata {
        seed_hash,
        note_count,
        tempo_bpm,
        duration_ms: (total_samples as f64 / SAMPLE_RATE as f64 * 1000.0).round() as u64,
        audio_sha256,
    })
}

fn write_wav_header(writer: &mut impl Write, data_size: u32) -> Result<()> {
    let byte_rate = SAMPLE_RATE * CHANNELS as u32 * BITS_PER_SAMPLE as u32 / 8;
    let block_align = CHANNELS * BITS_PER_SAMPLE / 8;
    writer.write_all(b"RIFF")?;
    writer.write_all(&(36 + data_size).to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16_u32.to_le_bytes())?;
    writer.write_all(&1_u16.to_le_bytes())?;
    writer.write_all(&CHANNELS.to_le_bytes())?;
    writer.write_all(&SAMPLE_RATE.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&BITS_PER_SAMPLE.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_size.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn same_seed_produces_same_audio() {
        let path_a =
            std::env::temp_dir().join(format!("oorush-piano-a-{}.wav", std::process::id()));
        let path_b =
            std::env::temp_dir().join(format!("oorush-piano-b-{}.wav", std::process::id()));
        let a = generate_from_seed(b"public-key-seed", &path_a).unwrap();
        let b = generate_from_seed(b"public-key-seed", &path_b).unwrap();
        assert_eq!(a, b);
        assert_eq!(fs::read(&path_a).unwrap(), fs::read(&path_b).unwrap());
        let wav = fs::read(&path_a).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let _ = fs::remove_file(path_a);
        let _ = fs::remove_file(path_b);
    }
}
