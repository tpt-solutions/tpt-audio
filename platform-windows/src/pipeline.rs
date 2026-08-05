use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tpt_audio_core::diagnostics::Diagnostics;
use tpt_audio_core::engine::{CHANNELS, FRAMES_PER_BUFFER, SAMPLE_RATE};
use tpt_audio_core::graph::Route;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

const FLOAT_SIZE: usize = 4;
const RECONNECT_BACKOFF: Duration = Duration::from_millis(250);

struct CaptureEndpoint {
    id: String,
    client: Option<IAudioClient>,
    capture: Option<IAudioCaptureClient>,
    buf_frames: u32,
    valid: bool,
    next_retry: Instant,
}

struct RenderEndpoint {
    id: String,
    client: Option<IAudioClient>,
    render: Option<IAudioRenderClient>,
    buf_frames: u32,
    valid: bool,
    next_retry: Instant,
}

pub struct AudioPipeline {
    running: Arc<AtomicBool>,
    routes: Arc<Mutex<Vec<Route>>>,
    diagnostics: Arc<Diagnostics>,
    pipeline_thread: Option<thread::JoinHandle<()>>,
}

impl AudioPipeline {
    pub fn new(diagnostics: Arc<Diagnostics>) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            routes: Arc::new(Mutex::new(Vec::new())),
            diagnostics,
            pipeline_thread: None,
        }
    }

    pub fn start(
        &mut self,
        source_device_ids: &[String],
        sink_device_ids: &[String],
        initial_routes: Vec<Route>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        *self.routes.lock().unwrap() = initial_routes;
        let routes = self.routes.clone();

        let sources = source_device_ids.to_vec();
        let sinks = sink_device_ids.to_vec();

        let diagnostics = self.diagnostics.clone();
        let handle = thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let result = unsafe { run_pipeline(&sources, &sinks, &running, &routes, &diagnostics) };

            if let Err(e) = result {
                eprintln!("AudioPipeline error: {}", e);
            }
        });

        self.pipeline_thread = Some(handle);
        Ok(())
    }

    pub fn update_routes(&self, new_routes: Vec<Route>) {
        if let Ok(mut r) = self.routes.lock() {
            *r = new_routes;
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.pipeline_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn run_pipeline(
    source_ids: &[String],
    sink_ids: &[String],
    running: &AtomicBool,
    routes: &Arc<Mutex<Vec<Route>>>,
    diagnostics: &Arc<Diagnostics>,
) -> Result<(), Box<dyn std::error::Error>> {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    let mut captures: Vec<CaptureEndpoint> = source_ids
        .iter()
        .map(|id| CaptureEndpoint {
            id: id.clone(),
            client: None,
            capture: None,
            buf_frames: FRAMES_PER_BUFFER as u32,
            valid: false,
            next_retry: Instant::now(),
        })
        .collect();
    let mut renders: Vec<RenderEndpoint> = sink_ids
        .iter()
        .map(|id| RenderEndpoint {
            id: id.clone(),
            client: None,
            render: None,
            buf_frames: FRAMES_PER_BUFFER as u32,
            valid: false,
            next_retry: Instant::now(),
        })
        .collect();

    while running.load(Ordering::SeqCst) {
        let current_routes = routes.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let active_routes: Vec<&Route> = current_routes
            .iter()
            .filter(|r| r.connected && !r.muted)
            .collect();

        let mut produced_data = false;

        let mut capture_buffers: std::collections::HashMap<String, Vec<f32>> =
            std::collections::HashMap::new();
        for cap in captures.iter_mut() {
            match pull_capture(&enumerator, cap, running, diagnostics) {
                CaptureStatus::Data(samples) => {
                    capture_buffers.insert(cap.id.clone(), samples);
                    produced_data = true;
                }
                CaptureStatus::Silence | CaptureStatus::Pending => {}
                CaptureStatus::Reconnected => {}
                CaptureStatus::Invalid => {}
            }
        }

        if !produced_data {
            if captures.is_empty() {
                thread::sleep(Duration::from_micros(500));
            } else {
                thread::sleep(Duration::from_micros(250));
            }
            continue;
        }

        let mut sink_buffers: std::collections::HashMap<String, Vec<f32>> =
            std::collections::HashMap::new();
        for r in &renders {
            sink_buffers.insert(
                r.id.clone(),
                vec![0.0f32; FRAMES_PER_BUFFER * CHANNELS as usize],
            );
        }

        for route in &active_routes {
            if let Some(cap_buf) = capture_buffers.get(&route.source.device_id) {
                if let Some(sink_buf) = sink_buffers.get_mut(&route.sink.device_id) {
                    let gain = route.gain;
                    for (i, sample) in sink_buf.iter_mut().enumerate() {
                        if i < cap_buf.len() {
                            *sample += cap_buf[i] * gain;
                        }
                    }
                }
            }
        }

        let buf_size = FRAMES_PER_BUFFER * CHANNELS as usize;
        for r in renders.iter_mut() {
            match push_render(&enumerator, r, &sink_buffers, buf_size, diagnostics) {
                RenderOutcome::Wrote(frames) => {
                    if frames == 0 && !r.valid {
                        continue;
                    }
                }
                RenderOutcome::Reconnected => {}
                RenderOutcome::Invalid => {}
            }
        }
    }

    for cap in captures.iter_mut() {
        if let Some(ref client) = cap.client {
            let _ = client.Stop();
        }
    }
    for r in renders.iter_mut() {
        if let Some(ref client) = r.client {
            let _ = client.Stop();
        }
    }

    Ok(())
}

enum CaptureStatus {
    Data(Vec<f32>),
    Silence,
    Pending,
    Reconnected,
    Invalid,
}

unsafe fn pull_capture(
    enumerator: &IMMDeviceEnumerator,
    cap: &mut CaptureEndpoint,
    _running: &AtomicBool,
    diagnostics: &Arc<Diagnostics>,
) -> CaptureStatus {
    if cap.valid {
        let (client, capture) = match (cap.client.as_ref(), cap.capture.as_ref()) {
            (Some(c), Some(cap_cli)) => (c, cap_cli),
            _ => {
                cap.valid = false;
                return CaptureStatus::Invalid;
            }
        };

        let mut pad: *mut u8 = std::ptr::null_mut();
        let mut frames: u32 = 0;
        let mut flags: u32 = 0;

        match capture.GetBuffer(&mut pad, &mut frames, &mut flags, None, None) {
            Ok(()) if !pad.is_null() && frames > 0 => {
                let sample_count = frames as usize * CHANNELS as usize;
                let samples = std::slice::from_raw_parts(pad as *const f32, sample_count);
                if (flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32) != 0 {
                    eprintln!(
                        "AudioPipeline: capture glitch (data discontinuity) on {}",
                        cap.id
                    );
                }
                let _ = capture.ReleaseBuffer(frames);
                CaptureStatus::Data(samples.to_vec())
            }
            Ok(()) => CaptureStatus::Silence,
            Err(e) if is_device_invalidated(&e) => {
                let _ = client.Stop();
                cap.valid = false;
                CaptureStatus::Invalid
            }
            Err(_) => CaptureStatus::Pending,
        }
    } else if cap.next_retry <= Instant::now() {
        cap.next_retry = Instant::now() + RECONNECT_BACKOFF;
        if let Some((client, capture, buf_frames)) = reopen_capture(enumerator, &cap.id) {
            cap.client = Some(client);
            cap.capture = Some(capture);
            cap.buf_frames = buf_frames;
            cap.valid = true;
            diagnostics.record_device_reconnect();
            eprintln!("AudioPipeline: capture device reconnected: {}", cap.id);
            CaptureStatus::Reconnected
        } else {
            CaptureStatus::Invalid
        }
    } else {
        CaptureStatus::Pending
    }
}

enum RenderOutcome {
    Wrote(u32),
    Reconnected,
    Invalid,
}

unsafe fn push_render(
    enumerator: &IMMDeviceEnumerator,
    r: &mut RenderEndpoint,
    sink_buffers: &std::collections::HashMap<String, Vec<f32>>,
    buf_size: usize,
    diagnostics: &Arc<Diagnostics>,
) -> RenderOutcome {
    if !r.valid {
        if r.next_retry <= Instant::now() {
            r.next_retry = Instant::now() + RECONNECT_BACKOFF;
            if let Some((client, render, buf_frames)) = reopen_render(enumerator, &r.id) {
                r.client = Some(client);
                r.render = Some(render);
                r.buf_frames = buf_frames;
                r.valid = true;
                diagnostics.record_device_reconnect();
                eprintln!("AudioPipeline: render device reconnected: {}", r.id);
                return RenderOutcome::Reconnected;
            }
        }
        return RenderOutcome::Invalid;
    }

    let (client, render) = match (r.client.as_ref(), r.render.as_ref()) {
        (Some(c), Some(ren)) => (c, ren),
        _ => {
            r.valid = false;
            return RenderOutcome::Invalid;
        }
    };

    let sink_buf = sink_buffers.get(&r.id);
    let frames_to_write = (sink_buf.map(|b| b.len()).unwrap_or(0) / CHANNELS as usize)
        .min(buf_size / CHANNELS as usize);

    let result = if frames_to_write > 0 {
        match render.GetBuffer(frames_to_write as u32) {
            Ok(pad) => {
                let dst = std::slice::from_raw_parts_mut(
                    pad as *mut f32,
                    frames_to_write * CHANNELS as usize,
                );
                if let Some(src) = sink_buf {
                    dst.copy_from_slice(&src[..dst.len()]);
                }
                let _ = render.ReleaseBuffer(frames_to_write as u32, 0);
                RenderOutcome::Wrote(frames_to_write as u32)
            }
            Err(e) if is_device_invalidated(&e) => {
                let _ = client.Stop();
                r.valid = false;
                RenderOutcome::Invalid
            }
            Err(_) => RenderOutcome::Wrote(0),
        }
    } else {
        match client.GetCurrentPadding() {
            Ok(padding) => {
                let available = r.buf_frames.saturating_sub(padding);
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
                RenderOutcome::Wrote(0)
            }
            Err(e) if is_device_invalidated(&e) => {
                let _ = client.Stop();
                r.valid = false;
                RenderOutcome::Invalid
            }
            Err(_) => RenderOutcome::Wrote(0),
        }
    };

    result
}

fn is_device_invalidated(e: &windows::core::Error) -> bool {
    e.code() == AUDCLNT_E_DEVICE_INVALIDATED || e.code() == AUDCLNT_E_RESOURCES_INVALIDATED
}

unsafe fn reopen_capture(
    enumerator: &IMMDeviceEnumerator,
    device_id: &str,
) -> Option<(IAudioClient, IAudioCaptureClient, u32)> {
    match open_capture(enumerator, device_id) {
        Ok((client, capture, buf_frames)) => {
            let _ = client.Start();
            Some((client, capture, buf_frames))
        }
        Err(_) => None,
    }
}

unsafe fn reopen_render(
    enumerator: &IMMDeviceEnumerator,
    device_id: &str,
) -> Option<(IAudioClient, IAudioRenderClient, u32)> {
    match open_render(enumerator, device_id) {
        Ok((client, render, buf_frames)) => {
            let _ = client.Start();
            Some((client, render, buf_frames))
        }
        Err(_) => None,
    }
}

unsafe fn open_capture(
    enumerator: &IMMDeviceEnumerator,
    device_id: &str,
) -> Result<(IAudioClient, IAudioCaptureClient, u32), Box<dyn std::error::Error>> {
    let device = find_device(enumerator, device_id)
        .ok_or_else(|| format!("Capture device not found: {}", device_id))?;

    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
    let format = client.GetMixFormat()?;

    let hns = (FRAMES_PER_BUFFER as i64) * 10_000_000i64 / SAMPLE_RATE as i64;
    client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK,
        hns,
        0,
        format,
        None,
    )?;

    let capture: IAudioCaptureClient = client.GetService()?;
    let buf_frames = client.GetBufferSize()?;
    Ok((client, capture, buf_frames))
}

unsafe fn open_render(
    enumerator: &IMMDeviceEnumerator,
    device_id: &str,
) -> Result<(IAudioClient, IAudioRenderClient, u32), Box<dyn std::error::Error>> {
    let device = find_device(enumerator, device_id)
        .ok_or_else(|| format!("Render device not found: {}", device_id))?;

    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
    let format = client.GetMixFormat()?;

    let hns = (FRAMES_PER_BUFFER as i64) * 10_000_000i64 / SAMPLE_RATE as i64;
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, hns, 0, format, None)?;

    let render: IAudioRenderClient = client.GetService()?;
    let buf_frames = client.GetBufferSize()?;
    Ok((client, render, buf_frames))
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
