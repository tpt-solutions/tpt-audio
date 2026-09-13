use tpt_audio_core::graph::{AudioDevice, AudioDirection};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

use windows::Win32::Media::Audio::EDataFlow;

fn device_name(id: &str, index: u32, flow: EDataFlow) -> String {
    let prefix = if flow == eRender { "Output" } else { "Input" };
    format!("{} {} ({})", prefix, index + 1, &id[..id.len().min(16)])
}

pub fn enumerate_all() -> Result<Vec<AudioDevice>, Box<dyn std::error::Error>> {
    let mut devices = Vec::new();

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        for &flow in &[eRender, eCapture] {
            let collection = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)?;
            let count = collection.GetCount()?;

            let default_device = enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok();

            for i in 0..count {
                let device = collection.Item(i)?;
                let id = match device.GetId() {
                    Ok(id) => id.to_string().unwrap_or_else(|_| format!("dev_{}", i)),
                    Err(_) => format!("dev_{}", i),
                };

                let name = device_name(&id, i, flow);

                let is_default = if let Some(ref default) = default_device {
                    if let Ok(def_id) = default.GetId() {
                        def_id.to_string().unwrap_or_default() == id
                    } else {
                        false
                    }
                } else {
                    false
                };

                let direction = if flow == eRender {
                    AudioDirection::Output
                } else {
                    AudioDirection::Input
                };

                devices.push(AudioDevice {
                    id,
                    name,
                    direction,
                    channels: 2,
                    is_default,
                });
            }
        }
    }

    Ok(devices)
}
