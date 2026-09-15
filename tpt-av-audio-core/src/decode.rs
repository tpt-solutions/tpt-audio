//! Audio decoding: a small [`Decoder`] abstraction backed by the
//! `tpt-cadence` codec suite, with a built-in WAV fallback.
//!
//! Built with the `cadence` feature (path dependencies on the sibling
//! `tpt-cadence` checkout), [`DecodeRegistry::with_builtins`] registers
//! cadence's real-time-safe decoders for **WAV, AIFF, and FLAC** and
//! decodes through the unified `tpt_av_cadence_core::Decoder` contract
//! (allocation-free `decode(&mut [f32])`). Without the feature, `.wav`
//! decodes via the built-in [`WavDecoder`] (hound) so CI and fresh clones
//! without the sibling checkout still build. Either way, extension
//! dispatch happens through [`DecodeRegistry`] (Main Thread only) and the
//! rest of the engine never sees a codec.

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

impl DecodedAudio {
    /// Number of complete frames in `data`.
    pub fn frames(&self) -> u64 {
        if self.channels == 0 {
            return 0;
        }
        (self.data.len() / self.channels as usize) as u64
    }
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
    /// Creates a registry preloaded with every decoder this build supports:
    /// the `tpt-cadence` suite (WAV/AIFF/FLAC) under the `cadence` feature,
    /// plus the built-in hound WAV decoder as the fallback.
    pub fn with_builtins() -> Self {
        let mut reg = Self::default();
        let wav = Arc::new(|path: &Path| Ok(Box::new(WavDecoder::open(path)?) as Box<dyn Decoder>));
        reg.register("wav", wav);

        #[cfg(feature = "cadence")]
        {
            use tpt_av_cadence_core::FormatReader;

            reg.register(
                "wav",
                Arc::new(|path| open_cadence(path, tpt_av_cadence_wav::WavReader::open)),
            );
            reg.register(
                "aiff",
                Arc::new(|path| open_cadence(path, tpt_av_cadence_aiff::AiffReader::open)),
            );
            reg.register(
                "aif",
                Arc::new(|path| open_cadence(path, tpt_av_cadence_aiff::AiffReader::open)),
            );
            reg.register(
                "flac",
                Arc::new(|path| open_cadence(path, tpt_av_cadence_flac::FlacReader::open)),
            );
        }
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

    /// Extensions this registry can decode, sorted (diagnostics/errors).
    pub fn supported_extensions(&self) -> Vec<&str> {
        let mut exts: Vec<&str> = self.factories.iter().map(|(e, _)| e.as_str()).collect();
        exts.sort_unstable();
        exts
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
                "no decoder registered for '.{ext}' (supported: {})",
                self.supported_extensions().join(", ")
            ))),
        }
    }
}

/// Adapter: opens a cadence [`tpt_av_cadence_core::FormatReader`] against
/// `path` and erases it into the engine's [`Decoder`] trait.
#[cfg(feature = "cadence")]
fn open_cadence<R, F>(path: &Path, open: F) -> Result<Box<dyn Decoder>, AudioError>
where
    R: tpt_av_cadence_core::FormatReader + 'static,
    F: FnOnce(Box<dyn std::io::Read + Send>) -> tpt_av_cadence_core::Result<R>,
{
    let file = std::fs::File::open(path).map_err(AudioError::Io)?;
    let reader = open(Box::new(file)).map_err(cadence_err)?;
    Ok(Box::new(CadenceDecoder { reader }))
}

/// Wraps a cadence format reader so its streaming, real-time-safe
/// [`tpt_av_cadence_core::Decoder::decode`] loop satisfies the engine's
/// whole-file [`Decoder::decode_all`] contract. Worker threads only.
#[cfg(feature = "cadence")]
struct CadenceDecoder<R: tpt_av_cadence_core::FormatReader + 'static> {
    reader: R,
}

#[cfg(feature = "cadence")]
impl<R: tpt_av_cadence_core::FormatReader + 'static> Decoder for CadenceDecoder<R> {
    fn decode_all(&mut self) -> Result<DecodedAudio, AudioError> {
        let info = self.reader.info().clone();
        let channels = info.channels.max(1) as usize;

        let mut data = Vec::new();
        let mut buf = vec![0.0f32; 8_192 * channels];
        loop {
            let frames = tpt_av_cadence_core::Decoder::decode(self.reader.decoder(), &mut buf)
                .map_err(cadence_err)?;
            if frames == 0 {
                break;
            }
            data.extend_from_slice(&buf[..frames * channels]);
        }

        Ok(DecodedAudio {
            sample_rate: info.sample_rate,
            channels: info.channels,
            data,
        })
    }
}

/// Maps a cadence error onto the engine error type.
#[cfg(feature = "cadence")]
fn cadence_err(e: tpt_av_cadence_core::CadenceError) -> AudioError {
    AudioError::Decode(e.to_string())
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
    fn unsupported_extension_lists_supported_ones() {
        let e = DecodeRegistry::with_builtins()
            .open(Path::new("song.xyz"))
            .err()
            .expect("no .xyz decoder exists");
        assert!(e.to_string().contains("no decoder registered for '.xyz'"));
        assert!(e.to_string().contains("wav"));
    }

    #[cfg(feature = "cadence")]
    #[test]
    fn cadence_decodes_wav_16_bit() {
        let dir = std::env::temp_dir().join("tpt-av-audio-core-cadence-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cadence16.wav");

        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        // 10 frames of L=100, R=-300 (raw i16).
        let mut samples = Vec::new();
        for _ in 0..10 {
            samples.push(100i16);
            samples.push(-300i16);
        }
        write_wav(&path, &samples, spec);

        let decoded = decode_file(&DecodeRegistry::with_builtins(), &path).unwrap();
        assert_eq!(decoded.sample_rate, 48_000);
        assert_eq!(decoded.channels, 2);
        // cadence round-trips the exact interleaved samples.
        assert_eq!(decoded.data.len(), 20);
        assert!((decoded.data[0] - 100.0 / 32_768.0).abs() < 1e-9);
        assert!((decoded.data[1] + 300.0 / 32_768.0).abs() < 1e-9);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "cadence")]
    #[test]
    fn cadence_decodes_wav_24_bit() {
        // 24-bit is in cadence's wheelhouse but not hound's reader API —
        // the whole reason cadence wins when enabled.
        let dir = std::env::temp_dir().join("tpt-av-audio-core-cadence-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cadence24.wav");

        // Hand-roll a minimal 24-bit PCM WAV (hound cannot write 24-bit).
        let frames: [i32; 4] = [0, 1_000_000, -1_000_000, 8_388_607];
        let mut pcm = Vec::new();
        for v in frames {
            pcm.extend_from_slice(&v.to_le_bytes()[..3]);
        }
        let header_len = 44usize;
        let data_len = pcm.len() as u32;
        let mut bytes = Vec::with_capacity(header_len + pcm.len());
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&144_000u32.to_le_bytes()); // byte rate
        bytes.extend_from_slice(&3u16.to_le_bytes()); // block align
        bytes.extend_from_slice(&24u16.to_le_bytes()); // bits
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend_from_slice(&pcm);
        std::fs::write(&path, &bytes).unwrap();

        let decoded = decode_file(&DecodeRegistry::with_builtins(), &path).unwrap();
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.data.len(), 4);
        // 8388607 / 8388608 ≈ full scale.
        assert!((decoded.data[3] - 1.0).abs() < 1e-3);
        assert!((decoded.data[2] + 1_000_000.0 / 8_388_608.0).abs() < 1e-6);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "cadence")]
    #[test]
    fn cadence_registers_aiff_and_flac() {
        let reg = DecodeRegistry::with_builtins();
        let exts = reg.supported_extensions();
        for ext in ["aif", "aiff", "flac", "wav"] {
            assert!(exts.contains(&ext), "missing {ext} in {exts:?}");
        }
        // Opening a non-audio file as FLAC reaches cadence's parser (Decode
        // error), not the registry's Unsupported.
        let dir = std::env::temp_dir().join("tpt-av-audio-core-cadence-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.flac");
        std::fs::write(&junk, b"not a flac file").unwrap();
        let e = decode_file(&reg, &junk).expect_err("junk must fail");
        assert!(!e.to_string().contains("no decoder registered"));
        let _ = std::fs::remove_file(&junk);
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
