use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Records audio from the default input device until the returned sender is
/// dropped, then resamples to 16 kHz mono f32 and sends the result.
pub fn start_recording() -> Result<(mpsc::Sender<()>, mpsc::Receiver<Vec<f32>>)> {
    let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
    let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<f32>>(1);

    std::thread::spawn(move || {
        if let Err(e) = record_until_stop(&mut stop_rx, pcm_tx) {
            tracing::error!("audio capture error: {}", e);
        }
    });

    Ok((stop_tx, pcm_rx))
}

fn record_until_stop(
    stop_rx: &mut mpsc::Receiver<()>,
    result_tx: mpsc::Sender<Vec<f32>>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default input device")?;

    let config = device
        .default_input_config()
        .context("no default input config")?;

    let input_sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let captured: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);

    let stream = device.build_input_stream(
        &config.config(),
        move |data: &[f32], _| {
            // Mix down to mono
            let mut buf = captured_clone.lock().unwrap();
            for frame in data.chunks(channels) {
                let mono = frame.iter().copied().sum::<f32>() / channels as f32;
                buf.push(mono);
            }
        },
        |e| tracing::error!("audio stream error: {}", e),
        None,
    )?;

    stream.play()?;

    // Block until stop signal
    let rt = tokio::runtime::Handle::current();
    rt.block_on(async { stop_rx.recv().await });

    drop(stream);

    let samples = Arc::try_unwrap(captured)
        .map_err(|_| anyhow::anyhow!("arc still shared after stream drop"))?
        .into_inner()?;

    let resampled = if input_sample_rate != TARGET_SAMPLE_RATE {
        resample_mono(samples, input_sample_rate, TARGET_SAMPLE_RATE)?
    } else {
        samples
    };

    let _ = result_tx.blocking_send(resampled);
    Ok(())
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
    let mut resampler =
        SincFixedIn::<f32>::new(ratio, 2.0, params, input.len(), 1)?;

    let waves_in = vec![input];
    let out = resampler.process(&waves_in, None)?;
    Ok(out.into_iter().next().unwrap_or_default())
}

/// Encodes a mono f32 PCM buffer at 16kHz into a WAV-formatted Vec<u8>.
/// The WAV header signals 16-bit PCM (converted from f32).
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
