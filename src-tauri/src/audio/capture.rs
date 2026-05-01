use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Returns the names of all available cpal input devices.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Records audio from the selected input device until the returned sender is
/// dropped, then resamples to 16 kHz mono f32 and sends the result.
pub fn start_recording(device_name: Option<String>) -> Result<(mpsc::Sender<()>, mpsc::Receiver<Vec<f32>>)> {
    let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
    let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<f32>>(1);

    let rt = tokio::runtime::Handle::current();

    std::thread::spawn(move || {
        if let Err(e) = record_until_stop(&mut stop_rx, pcm_tx, rt, device_name) {
            tracing::error!("audio capture error: {}", e);
        }
    });

    Ok((stop_tx, pcm_rx))
}

fn record_until_stop(
    stop_rx: &mut mpsc::Receiver<()>,
    result_tx: mpsc::Sender<Vec<f32>>,
    rt: tokio::runtime::Handle,
    device_name: Option<String>,
) -> Result<()> {
    let host = cpal::default_host();

    let device = match &device_name {
        Some(name) => {
            let found = host
                .input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(name.as_str()));
            match found {
                Some(d) => d,
                None => {
                    tracing::warn!(
                        "input device '{}' not found — falling back to system default",
                        name
                    );
                    host.default_input_device().context("no default input device")?
                }
            }
        }
        None => host.default_input_device().context("no default input device")?,
    };

    let device_name_str = device.name().unwrap_or_else(|_| "<unknown>".into());
    tracing::info!("recording on device: {}", device_name_str);

    let supported_config = device
        .default_input_config()
        .context("no default input config")?;

    let input_sample_rate = supported_config.sample_rate().0;
    let channels = supported_config.channels() as usize;
    let sample_format = supported_config.sample_format();
    let stream_config = supported_config.config();

    tracing::info!(
        "device config: {} Hz, {} ch, format={:?}",
        input_sample_rate, channels, sample_format
    );

    let captured: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

    // Build a stream that matches the device's native sample format.
    // Requesting f32 from a device that natively delivers i16 results in
    // CoreAudio silently returning zeros on many external USB mics.
    let stream = build_input_stream(&device, &stream_config, sample_format, channels, Arc::clone(&captured))?;

    stream.play()?;

    rt.block_on(async { stop_rx.recv().await });

    drop(stream);

    // Don't try_unwrap — the cpal callback closure may still hold a clone of
    // the Arc after drop(stream) returns. Just lock and drain directly.
    let samples = captured.lock().unwrap().drain(..).collect::<Vec<f32>>();

    if samples.is_empty() {
        tracing::warn!("no samples captured — microphone may not be accessible");
    } else if samples.iter().all(|&s| s == 0.0) {
        tracing::warn!(
            "captured {} samples but all are zero — mic may be muted or wrong device selected (format={:?})",
            samples.len(), sample_format
        );
    } else {
        tracing::debug!("captured {} samples at {} Hz", samples.len(), input_sample_rate);
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
        SampleFormat::F32 => {
            device.build_input_stream(
                config,
                move |data: &[f32], _| push_mono_f32(data, channels, &captured),
                err_fn,
                None,
            )?
        }
        SampleFormat::I16 => {
            device.build_input_stream(
                config,
                move |data: &[i16], _| {
                    let floats: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    push_mono_f32(&floats, channels, &captured);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::U16 => {
            device.build_input_stream(
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
            )?
        }
        _ => {
            tracing::warn!("unhandled sample format {:?}, attempting f32 fallback", sample_format);
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

/// Encodes a mono f32 PCM buffer at 16kHz into a WAV-formatted Vec<u8>.
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
