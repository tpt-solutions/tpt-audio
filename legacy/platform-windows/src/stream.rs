use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use tpt_audio_core::engine::{CHANNELS, FRAMES_PER_BUFFER, SAMPLE_RATE};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

pub const FLOAT_SIZE: usize = 4;

pub enum StreamCommand {
    SetGain(f32),
    SetMute(bool),
    Stop,
}

#[derive(Clone)]
pub struct StreamHandle {
    pub running: Arc<AtomicBool>,
    pub cmd_tx: Sender<StreamCommand>,
    pub audio_rx: Arc<Mutex<Receiver<Vec<f32>>>>,
}

pub struct CaptureStream {
    device_id: String,
    handle: Option<StreamHandle>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl CaptureStream {
    pub fn new(device_id: &str) -> Self {
        Self {
            device_id: device_id.to_string(),
            handle: None,
            thread_handle: None,
        }
    }

    pub fn start(&mut self) -> Result<StreamHandle, Box<dyn std::error::Error>> {
        let running = Arc::new(AtomicBool::new(true));
        let (cmd_tx, cmd_rx) = mpsc::channel::<StreamCommand>();
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
        let device_id = self.device_id.clone();
        let r = running.clone();

        let handle = thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let result = unsafe { run_capture_loop(&device_id, &r, &cmd_rx, &audio_tx) };
            if let Err(e) = result {
                eprintln!("CaptureStream[{}] error: {}", device_id, e);
            }
        });

        let stream_handle = StreamHandle {
            running,
            cmd_tx,
            audio_rx: Arc::new(Mutex::new(audio_rx)),
        };

        self.handle = Some(stream_handle.clone());
        self.thread_handle = Some(handle);
        Ok(stream_handle)
    }

    pub fn stop(&mut self) {
        if let Some(ref handle) = self.handle {
            handle.running.store(false, Ordering::SeqCst);
            let _ = handle.cmd_tx.send(StreamCommand::Stop);
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for CaptureStream {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn run_capture_loop(
    device_id: &str,
    running: &AtomicBool,
    cmd_rx: &Receiver<StreamCommand>,
    audio_tx: &Sender<Vec<f32>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = open_audio_client(device_id, true)?;
    let capture: IAudioCaptureClient = client.GetService()?;
    client.Start()?;

    let mut gain: f32 = 1.0;
    let mut muted: bool = false;

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                StreamCommand::SetGain(g) => gain = g.clamp(0.0, 1.0),
                StreamCommand::SetMute(m) => muted = m,
                StreamCommand::Stop => {
                    let _ = client.Stop();
                    return Ok(());
                }
            }
        }

        if !running.load(Ordering::SeqCst) {
            let _ = client.Stop();
            return Ok(());
        }

        let mut pad: *mut u8 = std::ptr::null_mut();
        let mut frames: u32 = 0;
        let mut flags: u32 = 0;

        let hr = capture.GetBuffer(&mut pad, &mut frames, &mut flags, None, None);
        if hr.is_ok() && !pad.is_null() && frames > 0 {
            let sample_count = frames as usize * CHANNELS as usize;
            let samples = std::slice::from_raw_parts(pad as *const f32, sample_count);

            let mut output = Vec::with_capacity(sample_count);
            if muted {
                output.resize(sample_count, 0.0);
            } else if (gain - 1.0).abs() > f32::EPSILON {
                output = samples.iter().map(|&s| s * gain).collect();
            } else {
                output = samples.to_vec();
            }

            let _ = audio_tx.send(output);
            let _ = capture.ReleaseBuffer(frames);
        }

        if frames == 0 {
            thread::sleep(std::time::Duration::from_micros(500));
        }
    }
}

pub struct RenderStream {
    device_id: String,
    handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
    audio_tx: Sender<Vec<f32>>,
}

impl RenderStream {
    pub fn new(device_id: &str) -> Self {
        let (audio_tx, _) = mpsc::channel();
        Self {
            device_id: device_id.to_string(),
            handle: None,
            running: Arc::new(AtomicBool::new(false)),
            audio_tx,
        }
    }

    pub fn start(&mut self) -> Result<Sender<Vec<f32>>, Box<dyn std::error::Error>> {
        self.running.store(true, Ordering::SeqCst);
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
        self.audio_tx = audio_tx.clone();
        let device_id = self.device_id.clone();
        let running = self.running.clone();

        let handle = thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let result = unsafe { run_render_loop(&device_id, &running, &audio_rx) };
            if let Err(e) = result {
                eprintln!("RenderStream[{}] error: {}", device_id, e);
            }
        });

        self.handle = Some(handle);
        Ok(audio_tx)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RenderStream {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn run_render_loop(
    device_id: &str,
    running: &AtomicBool,
    audio_rx: &Receiver<Vec<f32>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = open_audio_client(device_id, false)?;
    let render: IAudioRenderClient = client.GetService()?;
    let buffer_frames = client.GetBufferSize()?;
    client.Start()?;

    while running.load(Ordering::SeqCst) {
        match audio_rx.recv_timeout(std::time::Duration::from_millis(5)) {
            Ok(samples) => {
                let frames_needed = samples.len() / CHANNELS as usize;
                let frames_to_write = frames_needed.min(buffer_frames as usize);

                if let Ok(pad) = render.GetBuffer(frames_to_write as u32) {
                    let dst = std::slice::from_raw_parts_mut(
                        pad as *mut f32,
                        frames_to_write * CHANNELS as usize,
                    );
                    dst.copy_from_slice(&samples[..dst.len()]);
                    let _ = render.ReleaseBuffer(frames_to_write as u32, 0);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(padding) = client.GetCurrentPadding() {
                    let available = buffer_frames - padding;
                    if available > 0 {
                        if let Ok(pad) = render.GetBuffer(available) {
                            std::ptr::write_bytes(
                                pad,
                                0,
                                available as usize * CHANNELS as usize * FLOAT_SIZE,
                            );
                            let _ = render.ReleaseBuffer(available, 0);
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }

    let _ = client.Stop();
    Ok(())
}

unsafe fn open_audio_client(
    device_id: &str,
    is_capture: bool,
) -> Result<IAudioClient, Box<dyn std::error::Error>> {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    let device = find_device(&enumerator, device_id)
        .ok_or_else(|| format!("Device not found: {}", device_id))?;

    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    let format = client.GetMixFormat()?;

    let hns_duration = (FRAMES_PER_BUFFER as i64) * 10_000_000i64 / SAMPLE_RATE as i64;

    let stream_flags = if is_capture {
        AUDCLNT_STREAMFLAGS_LOOPBACK
    } else {
        0u32
    };

    client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        stream_flags,
        hns_duration,
        0,
        format,
        None,
    )?;

    Ok(client)
}

unsafe fn find_device(enumerator: &IMMDeviceEnumerator, device_id: &str) -> Option<IMMDevice> {
    for &flow in &[eRender, eCapture] {
        if let Ok(collection) = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) {
            if let Ok(count) = collection.GetCount() {
                for i in 0..count {
                    if let Ok(device) = collection.Item(i) {
                        if let Ok(id) = device.GetId() {
                            if id.to_string().unwrap_or_default() == device_id {
                                return Some(device);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
