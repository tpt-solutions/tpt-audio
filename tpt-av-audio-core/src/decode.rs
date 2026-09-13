//! Audio decoding: a small [`Decoder`] abstraction with a built-in WAV
//! decoder, ready to host `tpt-cadence` when that crate ships.
//!
//! `tpt-cadence` (the TPT AV stack codec suite) is a spec-only repository
//! today, so [`open_decoder`] dispatches by file extension: `.wav` decodes
//! via the built-in [`WavDecoder`] (hound), everything else returns
//! [`AudioError::Unsupported`]. When cadence lands, register it through
//! [`DecodeRegistry`] (Main Thread) without touching the rest of the engine.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hound::{SampleFormat as HoundFormat, WavReader};
use tpt_av_audio_utils::{AudioError, Sample};

/// Fully decoded audio, interleaved canonical f32.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved f32 samples in [-1.0, 1.0].
    pub data: Vec<f32>,
}

/// A stateful decoder opened against one media file.
pub trait Decoder: Send {
    /// Decodes the whole file to interleaved f32. Main Thread / worker
    /// threads only — never the audio thread.
    fn decode_all(&mut self) -> Result<DecodedAudio, AudioError>;
}

/// Registry of per-extension decoder factories.
#[derive(Default)]
pub struct DecodeRegistry {
    factories: Vec<(String, DecoderFactory)>,
}

/// A factory opening decoders for paths (thread-safe; used by the pool).
pub type DecoderFactory = Arc<dyn Fn(&Path) -> Result<Box<dyn Decoder>, AudioError> + Send + Sync>;

impl DecodeRegistry {
    /// Creates a registry preloaded with the built-in WAV decoder.
    pub fn with_builtins() -> Self {
        let mut reg = Self::default();
        reg.register(
            "wav",
            Arc::new(|path| Ok(Box::new(WavDecoder::open(path)?))),
        );
        reg
    }

    /// Registers a factory for a lowercase extension (without dot),
    /// replacing any existing entry.
    pub fn register(&mut self, ext: &str, factory: DecoderFactory) {
        if let Some(slot) = self.factories.iter_mut().find(|(e, _)| e == ext) {
            slot.1 = factory;
        } else {
            self.factories.push((ext.to_string(), factory));
        }
    }

    /// Opens a decoder for `path` by extension.
    pub fn open(&self, path: &Path) -> Result<Box<dyn Decoder>, AudioError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        match self.factories.iter().find(|(e, _)| *e == ext) {
            Some((_, factory)) => factory(path),
            None => Err(AudioError::Unsupported(format!(
                "no decoder registered for '.{ext}' (tpt-cadence integration pending)"
            ))),
        }
    }
}

/// Built-in WAV decoder (16-bit PCM, 24-bit PCM, and 32-bit float).
pub struct WavDecoder {
    path: PathBuf,
}

impl WavDecoder {
    /// Validates the WAV header eagerly.
    pub fn open(path: &Path) -> Result<Self, AudioError> {
        let reader = WavReader::open(path).map_err(hound_err)?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err(AudioError::Decode(format!(
                "degenerate WAV header in {}",
                path.display()
            )));
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Decoder for WavDecoder {
    fn decode_all(&mut self) -> Result<DecodedAudio, AudioError> {
        let mut reader = WavReader::open(&self.path).map_err(hound_err)?;
        let spec = reader.spec();
        let channels = spec.channels;
        let sample_rate = spec.sample_rate;

        let mut data = Vec::new();
        match spec.sample_format {
            HoundFormat::Float => {
                for sample in reader.samples::<f32>() {
                    data.push(sample.map_err(|e| AudioError::Decode(e.to_string()))?);
                }
            }
            HoundFormat::Int => match spec.bits_per_sample {
                8 => {
                    // hound yields 8-bit WAV as i8; scale symmetrically.
                    for s in reader.samples::<i8>() {
                        data.push(s.map_err(hound_err)? as f32 / 128.0);
                    }
                }
                16 => {
                    for s in reader.samples::<i16>() {
                        data.push(s.map_err(hound_err)?.to_f32());
                    }
                }
                24 | 32 => {
                    for s in reader.samples::<i32>() {
                        data.push(s.map_err(hound_err)?.to_f32());
                    }
                }
                bits => {
                    return Err(AudioError::Decode(format!(
                        "unsupported WAV bit depth {bits} in {}",
                        self.path.display()
                    )))
                }
            },
        }

        Ok(DecodedAudio {
            sample_rate,
            channels,
            data,
        })
    }
}

fn hound_err(e: hound::Error) -> AudioError {
    AudioError::Decode(e.to_string())
}

/// Convenience: decode a whole file through a registry.
pub fn decode_file(registry: &DecodeRegistry, path: &Path) -> Result<DecodedAudio, AudioError> {
    let mut decoder = registry.open(path)?;
    decoder.decode_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};

    fn write_wav(path: &Path, samples: &[i16], spec: WavSpec) {
        let mut writer = WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn decodes_16_bit_wav() {
        let dir = std::env::temp_dir().join("tpt-av-audio-core-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test16.wav");

        let spec = WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let samples: Vec<i16> = (0..200).map(|i| (i * 300 % 32_000) as i16).collect();
        write_wav(&path, &samples, spec);

        let decoded = decode_file(&DecodeRegistry::with_builtins(), &path).unwrap();
        assert_eq!(decoded.sample_rate, 48_000);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.data.len(), 200);
        // Sample 0 stays ~0 (i16 → f32 asymmetric scale).
        assert!(decoded.data[0].abs() < 1e-6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn decodes_float_wav() {
        let dir = std::env::temp_dir().join("tpt-av-audio-core-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("testf32.wav");

        let spec = WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for v in [0.25f32, -0.25, 0.5] {
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_file(&DecodeRegistry::with_builtins(), &path).unwrap();
        assert!((decoded.data[1] + 0.25).abs() < 1e-6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unsupported_extension_names_cadence() {
        let e = DecodeRegistry::with_builtins()
            .open(Path::new("song.flac"))
            .err()
            .expect("flac must be unsupported until cadence lands");
        assert!(e.to_string().contains("tpt-cadence"));
    }

    #[test]
    fn custom_registry_entries_replace_builtins() {
        let mut reg = DecodeRegistry::with_builtins();
        reg.register(
            "wav",
            Arc::new(|_path| Ok(Box::new(SyntheticDecoder) as Box<dyn Decoder>)),
        );
        let decoded = decode_file(&reg, Path::new("anything.wav")).unwrap();
        assert_eq!(decoded.data, vec![0.5]);
    }

    struct SyntheticDecoder;
    impl Decoder for SyntheticDecoder {
        fn decode_all(&mut self) -> Result<DecodedAudio, AudioError> {
            Ok(DecodedAudio {
                sample_rate: 48_000,
                channels: 1,
                data: vec![0.5],
            })
        }
    }
}
