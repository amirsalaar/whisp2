use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Returns the names of all available cpal input devices.
// cpal 0.17 deprecates `name()` in favor of `description()`/`id()`, but the
// stored config matches the user's selected device by this exact name string,
// so switching the key would invalidate existing device selections.
#[allow(deprecated)]
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// A live recording session. Carries the channels to stop capture and receive
/// the resampled PCM, plus what device actually ended up being used so the caller
/// can warn the user when their chosen mic wasn't the one we recorded from.
pub struct RecordingSession {
    /// Drop (or send on) this to stop capture and trigger the PCM send.
    pub stop_tx: mpsc::Sender<()>,
    /// Receives the final 16 kHz mono f32 samples once recording stops.
    pub pcm_rx: mpsc::Receiver<Vec<f32>>,
    /// Name of the device we actually opened (may differ from the requested one).
    pub device_name: String,
    /// True when the caller asked for a specific device by name but it wasn't
    /// present, so we fell back to the macOS system-default input instead.
    pub fell_back: bool,
}

/// Records audio from the selected input device until `stop_tx` is dropped, then
/// resamples to 16 kHz mono f32 and sends the result. The device is resolved
/// synchronously here (not on the capture thread) so the returned session can
/// report the actual device name and whether a fallback occurred.
#[allow(deprecated)] // device matched by `name()`; see list_input_devices
pub fn start_recording(device_name: Option<String>) -> Result<RecordingSession> {
    let host = cpal::default_host();

    let (device, fell_back) = match &device_name {
        Some(name) => {
            let found = host
                .input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(name.as_str()));
            match found {
                Some(d) => (d, false),
                None => {
                    tracing::warn!(
                        "input device '{}' not found — falling back to system default",
                        name
                    );
                    (
                        resolve_system_default(&host).context("no default input device")?,
                        true,
                    )
                }
            }
        }
        None => (
            resolve_system_default(&host).context("no default input device")?,
            false,
        ),
    };

    let actual_name = device.name().unwrap_or_else(|_| "<unknown>".into());
    tracing::info!("recording on device: {}", actual_name);

    let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
    let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<f32>>(1);

    let rt = tokio::runtime::Handle::current();

    std::thread::spawn(move || {
        if let Err(e) = record_until_stop(&mut stop_rx, pcm_tx, rt, device) {
            tracing::error!("audio capture error: {}", e);
        }
    });

    Ok(RecordingSession {
        stop_tx,
        pcm_rx,
        device_name: actual_name,
        fell_back,
    })
}

/// Resolves the device to record from when "System Default" is selected (or when a
/// named device was missing and we're falling back). On macOS the OS-reported default
/// input is frequently a *silent* virtual driver (Teams/Zoom loopback) or an idle
/// Continuity device (iPhone mic) — opening it yields pure-zero buffers, which is the
/// long-standing "System Default records nothing" bug. So we divert to the best real
/// physical mic when the default looks unreliable. On other platforms (and when the
/// default is already a real mic) this is just `default_input_device()`.
#[allow(deprecated)] // device matched by `name()`; see list_input_devices
fn resolve_system_default(host: &cpal::Host) -> Option<cpal::Device> {
    let os_default = host.default_input_device();

    #[cfg(target_os = "macos")]
    {
        let default_name = os_default.as_ref().and_then(|d| d.name().ok());

        // (name, is_reliable_transport) for every input device, per CoreAudio.
        let transports = macos_transport::input_device_reliability();
        let candidates: Vec<(String, bool)> = host
            .input_devices()
            .map(|it| {
                it.filter_map(|d| d.name().ok())
                    .map(|n| {
                        let reliable = transports.get(&n).copied().unwrap_or(false);
                        (n, reliable)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let default_pair = default_name
            .as_deref()
            .map(|n| (n, transports.get(n).copied().unwrap_or(false)));

        if let Some(target) = choose_system_default(default_pair, &candidates) {
            let found = host
                .input_devices()
                .ok()
                .and_then(|mut it| it.find(|d| d.name().ok().as_deref() == Some(target)));
            match found {
                // Only claim the substitution once we actually hold the device — the
                // device list can change between enumerations (e.g. a USB mic
                // unplugged), so logging before the lookup could falsely report a
                // switch we didn't make.
                Some(dev) => {
                    let from = default_name.as_deref().unwrap_or("<unknown>");
                    tracing::warn!(
                        "system-default input '{}' is a silent/virtual device — recording from '{}' instead",
                        from, target
                    );
                    return Some(dev);
                }
                None => {
                    tracing::warn!(
                        "substitute mic '{}' disappeared before it could be opened — using OS default",
                        target
                    );
                }
            }
        }
    }

    os_default
}

/// Pure selection logic (unit-tested). Given the OS default input as
/// `(name, is_reliable)` and all input devices as `(name, is_reliable)`, returns
/// `Some(name)` of a substitute physical mic when the default is unreliable and a
/// reliable alternative exists, or `None` to keep the OS default as-is.
#[cfg(target_os = "macos")]
fn choose_system_default<'a>(
    default: Option<(&str, bool)>,
    candidates: &'a [(String, bool)],
) -> Option<&'a str> {
    let (_default_name, default_reliable) = default?;
    if default_reliable {
        return None; // OS default is a real mic — trust it.
    }
    // Default is virtual/Continuity/unknown: pick the first reliable physical mic.
    candidates
        .iter()
        .find(|(_, reliable)| *reliable)
        .map(|(name, _)| name.as_str())
}

/// CoreAudio transport type identifies how a device connects. Built-in, USB,
/// Bluetooth, etc. deliver real audio; virtual loopback drivers and idle Continuity
/// devices report silence, so they are NOT reliable defaults to auto-select.
#[cfg(target_os = "macos")]
fn is_reliable_transport(transport: u32) -> bool {
    // kAudioDeviceTransportType* fourccs.
    const BUILT_IN: u32 = u32::from_be_bytes(*b"bltn");
    const USB: u32 = u32::from_be_bytes(*b"usb ");
    const BLUETOOTH: u32 = u32::from_be_bytes(*b"blue");
    const BLUETOOTH_LE: u32 = u32::from_be_bytes(*b"blea");
    const HDMI: u32 = u32::from_be_bytes(*b"hdmi");
    const DISPLAY_PORT: u32 = u32::from_be_bytes(*b"dprt");
    const THUNDERBOLT: u32 = u32::from_be_bytes(*b"thun");
    const PCI: u32 = u32::from_be_bytes(*b"pci ");
    const FIREWIRE: u32 = u32::from_be_bytes(*b"1394");
    const AIRPLAY: u32 = u32::from_be_bytes(*b"airp");
    const AVB: u32 = u32::from_be_bytes(*b"eavb");

    matches!(
        transport,
        BUILT_IN
            | USB
            | BLUETOOTH
            | BLUETOOTH_LE
            | HDMI
            | DISPLAY_PORT
            | THUNDERBOLT
            | PCI
            | FIREWIRE
            | AIRPLAY
            | AVB
    )
}

fn record_until_stop(
    stop_rx: &mut mpsc::Receiver<()>,
    result_tx: mpsc::Sender<Vec<f32>>,
    rt: tokio::runtime::Handle,
    device: cpal::Device,
) -> Result<()> {
    let supported_config = device
        .default_input_config()
        .context("no default input config")?;

    let input_sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels() as usize;
    let sample_format = supported_config.sample_format();
    let stream_config = supported_config.config();

    tracing::info!(
        "device config: {} Hz, {} ch, format={:?}",
        input_sample_rate,
        channels,
        sample_format
    );

    let captured: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

    // Build a stream that matches the device's native sample format.
    // Requesting f32 from a device that natively delivers i16 results in
    // CoreAudio silently returning zeros on many external USB mics.
    let stream = build_input_stream(
        &device,
        &stream_config,
        sample_format,
        channels,
        Arc::clone(&captured),
    )?;

    stream.play()?;

    rt.block_on(async { stop_rx.recv().await });

    // Stop the audio unit before drop so the mic indicator clears immediately.
    // (cpal 0.17 breaks the old StreamInner→Stream Arc cycle with a Weak ref +
    // explicit Drop, so drop alone already releases the unit — this just makes
    // the release deterministic rather than waiting on the final Arc going away.)
    if let Err(e) = stream.pause() {
        tracing::warn!("audio stream pause failed: {}", e);
    }
    drop(stream);

    let samples = captured.lock().unwrap().drain(..).collect::<Vec<f32>>();

    if samples.is_empty() {
        tracing::warn!("no samples captured — microphone may not be accessible");
    } else if samples.iter().all(|&s| s == 0.0) {
        tracing::warn!(
            "captured {} samples but all are zero — mic may be muted or wrong device selected (format={:?})",
            samples.len(), sample_format
        );
    } else {
        tracing::debug!(
            "captured {} samples at {} Hz",
            samples.len(),
            input_sample_rate
        );
    }

    let resampled = if input_sample_rate != TARGET_SAMPLE_RATE {
        resample_mono(samples, input_sample_rate, TARGET_SAMPLE_RATE)?
    } else {
        samples
    };

    let _ = result_tx.blocking_send(resampled);
    Ok(())
}

fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    captured: Arc<Mutex<Vec<f32>>>,
) -> Result<Stream> {
    let err_fn = |e| tracing::error!("audio stream error: {}", e);

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| push_mono_f32(data, channels, &captured),
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                let floats: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                push_mono_f32(&floats, channels, &captured);
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| {
                let floats: Vec<f32> = data
                    .iter()
                    .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                    .collect();
                push_mono_f32(&floats, channels, &captured);
            },
            err_fn,
            None,
        )?,
        _ => {
            tracing::warn!(
                "unhandled sample format {:?}, attempting f32 fallback",
                sample_format
            );
            device.build_input_stream(
                config,
                move |data: &[f32], _| push_mono_f32(data, channels, &captured),
                err_fn,
                None,
            )?
        }
    };

    Ok(stream)
}

#[inline]
fn push_mono_f32(data: &[f32], channels: usize, captured: &Arc<Mutex<Vec<f32>>>) {
    let mut buf = captured.lock().unwrap();
    let ch = channels.max(1);
    for frame in data.chunks(ch) {
        let mono = frame.iter().copied().sum::<f32>() / ch as f32;
        buf.push(mono);
    }
}

fn resample_mono(input: Vec<f32>, from: u32, to: u32) -> Result<Vec<f32>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    let ratio = to as f64 / from as f64;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, input.len(), 1)?;

    let waves_in = vec![input];
    let out = resampler.process(&waves_in, None)?;
    Ok(out.into_iter().next().unwrap_or_default())
}

pub fn encode_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let cursor = std::io::Cursor::new(&mut buf);
    let mut writer = hound::WavWriter::new(cursor, spec)?;
    for &sample in samples {
        let s = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(buf)
}

/// CoreAudio device enumeration used to tell real mics from silent virtual/Continuity
/// ones. Kept here (not in volume.rs) because it's only used to resolve the recording
/// device. Returns a name→is_reliable map keyed by the same name string cpal reports,
/// which matches CoreAudio's kAudioObjectPropertyName on macOS.
#[cfg(target_os = "macos")]
mod macos_transport {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr;

    const SYSTEM_OBJECT: u32 = 1;
    const PROP_DEVICES: u32 = u32::from_be_bytes(*b"dev#"); // kAudioHardwarePropertyDevices
    const PROP_NAME: u32 = u32::from_be_bytes(*b"lnam"); // kAudioObjectPropertyName
    const PROP_TRANSPORT: u32 = u32::from_be_bytes(*b"tran"); // kAudioDevicePropertyTransportType
    const PROP_STREAMS: u32 = u32::from_be_bytes(*b"stm#"); // kAudioDevicePropertyStreams
    const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    const SCOPE_INPUT: u32 = u32::from_be_bytes(*b"inpu");
    const ELEMENT_MAIN: u32 = 0;
    const CF_UTF8: u32 = 0x0800_0100;

    #[repr(C)]
    struct Addr {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            id: u32,
            addr: *const Addr,
            qsz: u32,
            q: *const c_void,
            out: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            id: u32,
            addr: *const Addr,
            qsz: u32,
            q: *const c_void,
            sz: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: *const c_void, buf: *mut u8, sz: isize, enc: u32) -> bool;
        fn CFRelease(s: *const c_void);
    }

    fn prop_size(id: u32, sel: u32, scope: u32) -> Option<u32> {
        let addr = Addr {
            selector: sel,
            scope,
            element: ELEMENT_MAIN,
        };
        let mut sz = 0u32;
        let r = unsafe { AudioObjectGetPropertyDataSize(id, &addr, 0, ptr::null(), &mut sz) };
        if r == 0 {
            Some(sz)
        } else {
            None
        }
    }

    fn get_u32(id: u32, sel: u32, scope: u32) -> Option<u32> {
        let addr = Addr {
            selector: sel,
            scope,
            element: ELEMENT_MAIN,
        };
        let mut v = 0u32;
        let mut sz = 4u32;
        let r = unsafe {
            AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                ptr::null(),
                &mut sz,
                &mut v as *mut u32 as *mut c_void,
            )
        };
        if r == 0 {
            Some(v)
        } else {
            None
        }
    }

    fn get_name(id: u32) -> Option<String> {
        let addr = Addr {
            selector: PROP_NAME,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut cf: *const c_void = ptr::null();
        let mut sz = std::mem::size_of::<*const c_void>() as u32;
        let r = unsafe {
            AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                ptr::null(),
                &mut sz,
                &mut cf as *mut _ as *mut c_void,
            )
        };
        if r != 0 || cf.is_null() {
            return None;
        }
        let mut buf = [0u8; 256];
        let ok = unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, CF_UTF8) };
        let name = if ok {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Some(String::from_utf8_lossy(&buf[..end]).into_owned())
        } else {
            None
        };
        unsafe { CFRelease(cf) };
        name
    }

    fn has_input_stream(id: u32) -> bool {
        prop_size(id, PROP_STREAMS, SCOPE_INPUT)
            .map(|sz| sz > 0)
            .unwrap_or(false)
    }

    /// Maps each input device's name to whether its CoreAudio transport type is a
    /// reliable (real, audio-producing) connection. Devices we can't read are absent.
    pub fn input_device_reliability() -> HashMap<String, bool> {
        let mut out = HashMap::new();
        let Some(sz) = prop_size(SYSTEM_OBJECT, PROP_DEVICES, SCOPE_GLOBAL) else {
            return out;
        };
        let count = (sz / 4) as usize;
        if count == 0 {
            return out;
        }
        let mut ids = vec![0u32; count];
        let addr = Addr {
            selector: PROP_DEVICES,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut sz_io = sz;
        let r = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &addr,
                0,
                ptr::null(),
                &mut sz_io,
                ids.as_mut_ptr() as *mut c_void,
            )
        };
        if r != 0 {
            return out;
        }
        // CoreAudio writes the actual byte count back into sz_io; if a device was
        // removed between the size query and this call, the tail of `ids` is still
        // zero (kAudioObjectUnknown). Bound iteration by the real count instead of
        // relying on those zero IDs being skipped downstream.
        let actual = (sz_io / 4) as usize;
        for id in ids.into_iter().take(actual) {
            if !has_input_stream(id) {
                continue;
            }
            let Some(name) = get_name(id) else { continue };
            let reliable = get_u32(id, PROP_TRANSPORT, SCOPE_GLOBAL)
                .map(super::is_reliable_transport)
                .unwrap_or(false);
            out.insert(name, reliable);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_encode_wav_roundtrip() {
        let samples: Vec<f32> = (0..160).map(|i| (i as f32 / 160.0) * 2.0 - 1.0).collect();
        let bytes = encode_wav(&samples).unwrap();
        let cursor = std::io::Cursor::new(&bytes);
        let reader = hound::WavReader::new(cursor).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000);
        assert_eq!(spec.channels, 1);
        assert_eq!(reader.len(), samples.len() as u32);
    }

    #[test]
    fn test_encode_wav_empty() {
        let bytes = encode_wav(&[]).unwrap();
        let cursor = std::io::Cursor::new(&bytes);
        let reader = hound::WavReader::new(cursor).unwrap();
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(reader.len(), 0);
    }

    #[test]
    fn test_resample_mono_length() {
        let input = vec![0.0f32; 44100];
        let output = resample_mono(input, 44100, 16000).unwrap();
        let expected = 16000usize;
        let tolerance = (expected as f32 * 0.05) as usize;
        assert!(
            output.len().abs_diff(expected) <= tolerance,
            "output len {} not within 5% of {}",
            output.len(),
            expected
        );
    }

    #[test]
    fn test_resample_mono_same_rate() {
        let input: Vec<f32> = (0..16000).map(|i| (i as f32).sin()).collect();
        let output = resample_mono(input.clone(), 16000, 16000).unwrap();
        let tolerance = (input.len() as f32 * 0.05) as usize;
        assert!(
            output.len().abs_diff(input.len()) <= tolerance,
            "output len {} not within 5% of {}",
            output.len(),
            input.len()
        );
    }

    #[test]
    fn test_push_mono_stereo_mix() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let data = [0.4f32, 0.8f32, -0.2f32, 0.6f32];
        push_mono_f32(&data, 2, &captured);
        let result = captured.lock().unwrap().clone();
        assert_eq!(result.len(), 2);
        assert!((result[0] - 0.6f32).abs() < 1e-6);
        assert!((result[1] - 0.2f32).abs() < 1e-6);
    }

    #[test]
    fn test_push_mono_mono_passthrough() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let data = [0.1f32, 0.5f32, -0.3f32];
        push_mono_f32(&data, 1, &captured);
        let result = captured.lock().unwrap().clone();
        assert_eq!(result, data.to_vec());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_choose_system_default_substitutes_silent_default() {
        // macOS picked a silent virtual loopback as the system default — we should
        // divert to the real built-in mic instead.
        let candidates = vec![
            ("Microsoft Teams Audio".to_string(), false),
            ("MacBook Pro Microphone".to_string(), true),
        ];
        assert_eq!(
            choose_system_default(Some(("Microsoft Teams Audio", false)), &candidates),
            Some("MacBook Pro Microphone"),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_choose_system_default_substitutes_idle_continuity() {
        // An idle iPhone Continuity mic as the default is silent — divert to built-in.
        let candidates = vec![
            ("Amirsalar's iPhone Microphone".to_string(), false),
            ("MacBook Pro Microphone".to_string(), true),
        ];
        assert_eq!(
            choose_system_default(Some(("Amirsalar's iPhone Microphone", false)), &candidates),
            Some("MacBook Pro Microphone"),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_choose_system_default_keeps_reliable_default() {
        // The OS default is already a real mic — don't second-guess it.
        let candidates = vec![
            ("MacBook Pro Microphone".to_string(), true),
            ("Microsoft Teams Audio".to_string(), false),
        ];
        assert_eq!(
            choose_system_default(Some(("MacBook Pro Microphone", true)), &candidates),
            None,
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_choose_system_default_no_reliable_alternative() {
        // Default is silent but there's no real mic to switch to — stay put and let
        // the dead-mic detector surface the problem rather than picking another dud.
        let candidates = vec![("Microsoft Teams Audio".to_string(), false)];
        assert_eq!(
            choose_system_default(Some(("Microsoft Teams Audio", false)), &candidates),
            None,
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_choose_system_default_unknown_default_is_left_alone() {
        // Couldn't read the OS default's transport — don't override.
        let candidates = vec![("MacBook Pro Microphone".to_string(), true)];
        assert_eq!(choose_system_default(None, &candidates), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_is_reliable_transport() {
        assert!(is_reliable_transport(u32::from_be_bytes(*b"bltn"))); // built-in
        assert!(is_reliable_transport(u32::from_be_bytes(*b"usb "))); // USB
        assert!(is_reliable_transport(u32::from_be_bytes(*b"blue"))); // Bluetooth
        assert!(!is_reliable_transport(u32::from_be_bytes(*b"virt"))); // virtual driver
        assert!(!is_reliable_transport(u32::from_be_bytes(*b"ccwd"))); // Continuity
    }
}
