//! Minimal WAV (RIFF/WAVE) reader and writer.
//!
//! This is not a general-purpose WAV library: it covers exactly the formats
//! this workspace produces and consumes without `cadence` enabled — 8/16-bit
//! integer PCM plus 24-bit integer PCM (3- or 4-byte container) and 32-bit
//! IEEE float, canonical (non-extensible) `fmt ` chunks, any channel count.
//! For decoding a broader range of real-world WAV files (extensible headers,
//! more exotic bit depths), see `tpt-av-cadence-wav`, opt-in via the
//! `cadence` feature elsewhere in the workspace.
//!
//! The API mirrors the shape of the `hound` crate it replaces (`WavSpec`,
//! `WavWriter::create`/`write_sample`/`finalize`, `WavReader::open`/`spec`/
//! `samples::<S>()`) purely so callers changed little when hound (an
//! Apache-2.0-only crate) was dropped from the dependency tree.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::Path;

use crate::AudioError;

fn io_err(e: std::io::Error) -> AudioError {
    AudioError::Io(e)
}

fn decode_err(msg: impl Into<String>) -> AudioError {
    AudioError::Decode(msg.into())
}

/// Whether samples are stored as integers or IEEE floats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    Int,
    Float,
}

/// The format of a WAV file's audio data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavSpec {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub sample_format: SampleFormat,
}

/// A PCM sample width `WavReader`/`WavWriter` can read or write.
pub trait WavSample: Copy {
    fn write<W: Write>(self, w: &mut W, spec: &WavSpec) -> Result<(), AudioError>;
    fn read<R: Read>(r: &mut R, spec: &WavSpec, container_bytes: u16) -> Result<Self, AudioError>;
}

fn read_exact<R: Read, const N: usize>(r: &mut R) -> Result<[u8; N], AudioError> {
    let mut buf = [0u8; N];
    r.read_exact(&mut buf).map_err(io_err)?;
    Ok(buf)
}

fn biased_i8<R: Read>(r: &mut R) -> Result<i8, AudioError> {
    let [b] = read_exact::<_, 1>(r)?;
    Ok((b as i16 - 128) as i8)
}

fn sign_extend_i24(bytes: [u8; 3]) -> i32 {
    let x = u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16);
    if x & (1 << 23) == 0 {
        x as i32
    } else {
        (x | 0xff_00_00_00) as i32
    }
}

impl WavSample for i8 {
    fn write<W: Write>(self, w: &mut W, spec: &WavSpec) -> Result<(), AudioError> {
        match spec.bits_per_sample {
            8 => w.write_all(&[((self as i16) + 128) as u8]).map_err(io_err),
            bits => Err(decode_err(format!("cannot write i8 sample at {bits} bits"))),
        }
    }

    fn read<R: Read>(r: &mut R, spec: &WavSpec, container_bytes: u16) -> Result<Self, AudioError> {
        if spec.sample_format != SampleFormat::Int {
            return Err(decode_err("expected integer PCM sample format"));
        }
        match (container_bytes, spec.bits_per_sample) {
            (1, 8) => biased_i8(r),
            (n, bits) => Err(decode_err(format!(
                "cannot read {bits}-bit sample (container {n} bytes) as i8"
            ))),
        }
    }
}

impl WavSample for i16 {
    fn write<W: Write>(self, w: &mut W, spec: &WavSpec) -> Result<(), AudioError> {
        match spec.bits_per_sample {
            16 => w.write_all(&self.to_le_bytes()).map_err(io_err),
            bits => Err(decode_err(format!("cannot write i16 sample at {bits} bits"))),
        }
    }

    fn read<R: Read>(r: &mut R, spec: &WavSpec, container_bytes: u16) -> Result<Self, AudioError> {
        if spec.sample_format != SampleFormat::Int {
            return Err(decode_err("expected integer PCM sample format"));
        }
        match (container_bytes, spec.bits_per_sample) {
            (1, 8) => Ok(biased_i8(r)? as i16),
            (2, 16) => Ok(i16::from_le_bytes(read_exact(r)?)),
            (n, bits) => Err(decode_err(format!(
                "cannot read {bits}-bit sample (container {n} bytes) as i16"
            ))),
        }
    }
}

impl WavSample for i32 {
    fn write<W: Write>(self, w: &mut W, spec: &WavSpec) -> Result<(), AudioError> {
        match spec.bits_per_sample {
            16 => w.write_all(&(self as i16).to_le_bytes()).map_err(io_err),
            24 => w.write_all(&self.to_le_bytes()[..3]).map_err(io_err),
            32 => w.write_all(&self.to_le_bytes()).map_err(io_err),
            bits => Err(decode_err(format!("cannot write i32 sample at {bits} bits"))),
        }
    }

    fn read<R: Read>(r: &mut R, spec: &WavSpec, container_bytes: u16) -> Result<Self, AudioError> {
        if spec.sample_format != SampleFormat::Int {
            return Err(decode_err("expected integer PCM sample format"));
        }
        match (container_bytes, spec.bits_per_sample) {
            (1, 8) => Ok(biased_i8(r)? as i32),
            (2, 16) => Ok(i16::from_le_bytes(read_exact(r)?) as i32),
            (3, 24) => Ok(sign_extend_i24(read_exact(r)?)),
            (4, 24) => {
                let bytes: [u8; 4] = read_exact(r)?;
                Ok(sign_extend_i24([bytes[0], bytes[1], bytes[2]]))
            }
            (4, 32) => Ok(i32::from_le_bytes(read_exact(r)?)),
            (n, bits) => Err(decode_err(format!(
                "cannot read {bits}-bit sample (container {n} bytes) as i32"
            ))),
        }
    }
}

impl WavSample for f32 {
    fn write<W: Write>(self, w: &mut W, spec: &WavSpec) -> Result<(), AudioError> {
        match spec.bits_per_sample {
            32 => w.write_all(&self.to_le_bytes()).map_err(io_err),
            bits => Err(decode_err(format!("cannot write f32 sample at {bits} bits"))),
        }
    }

    fn read<R: Read>(r: &mut R, spec: &WavSpec, container_bytes: u16) -> Result<Self, AudioError> {
        if spec.sample_format != SampleFormat::Float {
            return Err(decode_err("expected IEEE float sample format"));
        }
        match (container_bytes, spec.bits_per_sample) {
            (4, 32) => Ok(f32::from_le_bytes(read_exact(r)?)),
            (n, bits) => Err(decode_err(format!(
                "cannot read {bits}-bit sample (container {n} bytes) as f32"
            ))),
        }
    }
}

/// Writes WAV files sample-by-sample, patching the RIFF/data sizes on
/// [`WavWriter::finalize`].
pub struct WavWriter<W: Write + Seek> {
    writer: W,
    spec: WavSpec,
    data_start: u64,
    bytes_written: u32,
}

impl WavWriter<BufWriter<File>> {
    /// Creates `path`, truncating it, and writes a placeholder header.
    pub fn create(path: impl AsRef<Path>, spec: WavSpec) -> Result<Self, AudioError> {
        let file = File::create(path).map_err(io_err)?;
        Self::new(BufWriter::new(file), spec)
    }
}

impl<W: Write + Seek> WavWriter<W> {
    pub fn new(mut writer: W, spec: WavSpec) -> Result<Self, AudioError> {
        let format_tag: u16 = match spec.sample_format {
            SampleFormat::Int => 1,
            SampleFormat::Float => 3,
        };
        let block_align = spec.channels * (spec.bits_per_sample / 8);
        let byte_rate = spec.sample_rate * block_align as u32;

        writer.write_all(b"RIFF").map_err(io_err)?;
        writer.write_all(&0u32.to_le_bytes()).map_err(io_err)?; // patched in finalize
        writer.write_all(b"WAVE").map_err(io_err)?;

        writer.write_all(b"fmt ").map_err(io_err)?;
        writer.write_all(&16u32.to_le_bytes()).map_err(io_err)?;
        writer.write_all(&format_tag.to_le_bytes()).map_err(io_err)?;
        writer.write_all(&spec.channels.to_le_bytes()).map_err(io_err)?;
        writer.write_all(&spec.sample_rate.to_le_bytes()).map_err(io_err)?;
        writer.write_all(&byte_rate.to_le_bytes()).map_err(io_err)?;
        writer.write_all(&block_align.to_le_bytes()).map_err(io_err)?;
        writer
            .write_all(&spec.bits_per_sample.to_le_bytes())
            .map_err(io_err)?;

        writer.write_all(b"data").map_err(io_err)?;
        writer.write_all(&0u32.to_le_bytes()).map_err(io_err)?; // patched in finalize

        let data_start = writer.stream_position().map_err(io_err)?;
        Ok(Self {
            writer,
            spec,
            data_start,
            bytes_written: 0,
        })
    }

    pub fn write_sample<S: WavSample>(&mut self, sample: S) -> Result<(), AudioError> {
        sample.write(&mut self.writer, &self.spec)?;
        self.bytes_written += (self.spec.bits_per_sample / 8) as u32;
        Ok(())
    }

    /// Patches the RIFF and `data` chunk sizes and flushes to disk.
    pub fn finalize(mut self) -> Result<(), AudioError> {
        self.writer.flush().map_err(io_err)?;
        let riff_size = 36 + self.bytes_written;

        self.writer.seek(SeekFrom::Start(4)).map_err(io_err)?;
        self.writer
            .write_all(&riff_size.to_le_bytes())
            .map_err(io_err)?;

        self.writer
            .seek(SeekFrom::Start(self.data_start - 4))
            .map_err(io_err)?;
        self.writer
            .write_all(&self.bytes_written.to_le_bytes())
            .map_err(io_err)?;

        self.writer.flush().map_err(io_err)
    }
}

/// Reads WAV files sample-by-sample via [`WavReader::samples`].
pub struct WavReader<R> {
    reader: R,
    spec: WavSpec,
    container_bytes: u16,
    samples_read: u32,
    total_samples: u32,
}

impl WavReader<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AudioError> {
        let file = File::open(path).map_err(io_err)?;
        Self::new(BufReader::new(file))
    }
}

impl<R: Read> WavReader<R> {
    pub fn new(mut reader: R) -> Result<Self, AudioError> {
        let tag: [u8; 4] = read_exact(&mut reader)?;
        if &tag != b"RIFF" {
            return Err(decode_err("not a RIFF file"));
        }
        let _riff_size = u32::from_le_bytes(read_exact(&mut reader)?);
        let wave: [u8; 4] = read_exact(&mut reader)?;
        if &wave != b"WAVE" {
            return Err(decode_err("RIFF file is not WAVE"));
        }

        let mut spec = None;
        let mut container_bytes = 0u16;
        let mut total_samples = None;

        while total_samples.is_none() {
            let id: [u8; 4] = read_exact(&mut reader)?;
            let size = u32::from_le_bytes(read_exact(&mut reader)?);
            match &id {
                b"fmt " => {
                    if size < 16 {
                        return Err(decode_err("fmt chunk shorter than 16 bytes"));
                    }
                    let format_tag = u16::from_le_bytes(read_exact(&mut reader)?);
                    let channels = u16::from_le_bytes(read_exact(&mut reader)?);
                    let sample_rate = u32::from_le_bytes(read_exact(&mut reader)?);
                    let _byte_rate = u32::from_le_bytes(read_exact(&mut reader)?);
                    let block_align = u16::from_le_bytes(read_exact(&mut reader)?);
                    let bits_per_sample = u16::from_le_bytes(read_exact(&mut reader)?);
                    skip(&mut reader, size as u64 - 16)?;

                    if channels == 0 {
                        return Err(decode_err("WAV header declares 0 channels"));
                    }
                    let sample_format = match format_tag {
                        1 => SampleFormat::Int,
                        3 => SampleFormat::Float,
                        other => {
                            return Err(decode_err(format!(
                                "unsupported WAV format tag {other} (only PCM and IEEE float)"
                            )))
                        }
                    };
                    container_bytes = block_align / channels;
                    spec = Some(WavSpec {
                        channels,
                        sample_rate,
                        bits_per_sample,
                        sample_format,
                    });
                }
                b"data" => {
                    let spec = spec
                        .ok_or_else(|| decode_err("WAV data chunk appeared before fmt chunk"))?;
                    if container_bytes == 0 {
                        return Err(decode_err("WAV header declares 0 block align"));
                    }
                    total_samples = Some(size / container_bytes as u32);
                    return Ok(Self {
                        reader,
                        spec,
                        container_bytes,
                        samples_read: 0,
                        total_samples: total_samples.unwrap(),
                    });
                }
                _ => skip(&mut reader, size as u64)?,
            }
            if size % 2 == 1 {
                skip(&mut reader, 1)?; // RIFF chunks are word-aligned
            }
        }
        unreachable!()
    }

    pub fn spec(&self) -> WavSpec {
        self.spec
    }

    /// Duration in frames (independent of channel count).
    pub fn duration(&self) -> u32 {
        self.total_samples / self.spec.channels as u32
    }

    pub fn samples<S: WavSample>(&mut self) -> WavSamples<'_, R, S> {
        WavSamples {
            reader: self,
            _marker: PhantomData,
        }
    }
}

fn skip<R: Read>(r: &mut R, n: u64) -> Result<(), AudioError> {
    std::io::copy(&mut r.by_ref().take(n), &mut std::io::sink())
        .map_err(io_err)
        .map(|_| ())
}

/// Streaming sample iterator returned by [`WavReader::samples`].
pub struct WavSamples<'r, R, S> {
    reader: &'r mut WavReader<R>,
    _marker: PhantomData<S>,
}

impl<'r, R: Read, S: WavSample> Iterator for WavSamples<'r, R, S> {
    type Item = Result<S, AudioError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.reader.samples_read >= self.reader.total_samples {
            return None;
        }
        self.reader.samples_read += 1;
        Some(S::read(
            &mut self.reader.reader,
            &self.reader.spec,
            self.reader.container_bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_spec(spec: WavSpec, samples: &[i32]) -> Vec<i32> {
        let dir = std::env::temp_dir().join("tpt-av-audio-utils-wav-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "roundtrip-{}-{:?}.wav",
            spec.bits_per_sample, spec.sample_format
        ));

        let mut writer = WavWriter::create(&path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();

        let mut reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.spec(), spec);
        let out: Vec<i32> = reader.samples::<i32>().map(|s| s.unwrap()).collect();
        let _ = std::fs::remove_file(&path);
        out
    }

    #[test]
    fn round_trips_16_bit_pcm() {
        let spec = WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let samples = vec![0, 16_000, -16_000, 32_767, -32_768];
        assert_eq!(round_trip_spec(spec, &samples), samples);
    }

    #[test]
    fn round_trips_24_bit_pcm() {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        };
        let samples = vec![0, 8_388_607, -8_388_608, 1_234_567];
        assert_eq!(round_trip_spec(spec, &samples), samples);
    }

    #[test]
    fn round_trips_float32() {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };

        let dir = std::env::temp_dir().join("tpt-av-audio-utils-wav-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roundtrip-float.wav");

        let mut writer = WavWriter::create(&path, spec).unwrap();
        for v in [0.25f32, -0.5, 0.999] {
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();

        let mut reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.spec(), spec);
        let out: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(out, vec![0.25, -0.5, 0.999]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_non_riff_data() {
        let result = WavReader::new(std::io::Cursor::new(b"not a wav".to_vec()));
        let err = match result {
            Ok(_) => panic!("expected an error for non-RIFF data"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("RIFF"));
    }
}
