//! Plays a brief two-tone completion chime on the default output device.
//! Runs on a dedicated thread to avoid blocking the async runtime.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub fn play() {
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => return,
    };
    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(_) => return,
    };

    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;

    // Two tones: 880 Hz for 80ms, then 1047 Hz for 120ms
    let tone1_samples = (sample_rate * 0.08) as usize;
    let tone2_samples = (sample_rate * 0.12) as usize;
    let total_samples = tone1_samples + tone2_samples;

    let samples: Vec<f32> = (0..total_samples)
        .map(|i| {
            let t = i as f32 / sample_rate;
            let freq = if i < tone1_samples { 880.0 } else { 1047.0 };
            let local_i = if i < tone1_samples { i } else { i - tone1_samples };
            let local_total = if i < tone1_samples { tone1_samples } else { tone2_samples };
            let fade = 1.0 - (local_i as f32 / local_total as f32);
            0.3 * fade * (2.0 * std::f32::consts::PI * freq * t).sin()
        })
        .collect();

    let mut cursor = 0usize;
    let samples = std::sync::Arc::new(samples);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_clone = std::sync::Arc::clone(&done);

    let stream_result = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let samples_clone = std::sync::Arc::clone(&samples);
            device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    for frame in data.chunks_mut(channels) {
                        let s = if cursor < samples_clone.len() {
                            let v = samples_clone[cursor];
                            cursor += 1;
                            v
                        } else {
                            done_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                            0.0
                        };
                        for ch in frame.iter_mut() {
                            *ch = s;
                        }
                    }
                },
                |e| tracing::warn!("sound stream error: {}", e),
                None,
            )
        }
        _ => return,
    };

    let stream = match stream_result {
        Ok(s) => s,
        Err(_) => return,
    };

    if stream.play().is_err() {
        return;
    }

    // Wait until all samples played (max 500ms)
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while !done.load(std::sync::atomic::Ordering::SeqCst) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(stream);
}
