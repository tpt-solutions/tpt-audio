use std::collections::HashMap;

use tpt_audio_core::graph::AudioSource;
use windows::core::Interface;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

pub fn enumerate_audio_sessions() -> Result<Vec<AudioSource>, Box<dyn std::error::Error>> {
    let mut sources = Vec::new();
    let mut seen: HashMap<String, u32> = HashMap::new();

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let count = collection.GetCount()?;

        for i in 0..count {
            let device = collection.Item(i)?;

            let session_manager: Result<IAudioSessionManager2, _> =
                device.Activate(CLSCTX_ALL, None);

            if let Ok(manager) = session_manager {
                if let Ok(enumerator) = manager.GetSessionEnumerator() {
                    let session_count = enumerator.GetCount()?;

                    for j in 0..session_count {
                        let control = enumerator.GetSession(j)?;

                        let control2: Result<IAudioSessionControl2, _> = control.cast();
                        if let Ok(ctrl2) = control2 {
                            let pid = match ctrl2.GetProcessId() {
                                Ok(p) => p,
                                Err(_) => continue,
                            };
                            if pid == 0 {
                                continue;
                            }

                            let display_name = match ctrl2.GetDisplayName() {
                                Ok(name) => name.to_string().unwrap_or_default(),
                                Err(_) => String::new(),
                            };

                            let app_name = if display_name.is_empty() {
                                match ctrl2.GetSessionIdentifier() {
                                    Ok(id) => id
                                        .to_string()
                                        .unwrap_or_else(|_| format!("session_{}", pid)),
                                    Err(_) => format!("session_{}", pid),
                                }
                            } else {
                                display_name
                            };

                            let key = format!("{}_{}", app_name, pid);
                            if seen.contains_key(&key) {
                                continue;
                            }
                            seen.insert(key, pid);

                            sources.push(AudioSource {
                                device_id: format!("app::{}", pid),
                                app_name: Some(app_name),
                                app_pid: Some(pid),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(sources)
}

pub fn set_session_volume(pid: u32, volume: f32) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let count = collection.GetCount()?;

        for i in 0..count {
            let device = collection.Item(i)?;

            let session_manager: Result<IAudioSessionManager2, _> =
                device.Activate(CLSCTX_ALL, None);

            if let Ok(manager) = session_manager {
                if let Ok(enumerator) = manager.GetSessionEnumerator() {
                    let session_count = enumerator.GetCount()?;

                    for j in 0..session_count {
                        let control = enumerator.GetSession(j)?;
                        let control2: Result<IAudioSessionControl2, _> = control.cast();

                        if let Ok(ctrl2) = control2 {
                            let session_pid = match ctrl2.GetProcessId() {
                                Ok(p) => p,
                                Err(_) => continue,
                            };

                            if session_pid == pid {
                                let volume_obj: Result<ISimpleAudioVolume, _> = control.cast();
                                if let Ok(vol) = volume_obj {
                                    vol.SetMasterVolume(volume.clamp(0.0, 1.0), std::ptr::null())?;
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err(format!("No session found for PID {}", pid).into())
}
