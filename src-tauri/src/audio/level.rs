//! Live microphone level for the floating island's waveform.
//!
//! The cpal callback hands us raw buffers hundreds of times a second, which is far
//! more than a 24fps waveform can use and far more than we want to push across the
//! Tauri IPC boundary. `LevelMeter` turns those buffers into a smoothed 0..=1 level
//! and rate-limits how often the caller is asked to publish one.
//!
//! Kept free of Tauri types so it can be unit-tested and so the audio callback
//! stays ignorant of the UI: the caller supplies the sink.

use std::time::{Duration, Instant};

/// Minimum gap between published levels. ~20/sec is comfortably above the
/// waveform's own 24fps redraw and well below the callback rate.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(50);

/// How fast the displayed level chases the measured one. Raw RMS is jittery enough
/// to make the bars twitch; asymmetric smoothing (fast attack, slow release) makes
/// speech onset feel immediate while decay stays smooth.
const ATTACK: f32 = 0.5;
const RELEASE: f32 = 0.15;

/// Speech RMS is small in absolute terms — normal talking sits around 0.05–0.15.
/// Scaling by this maps that range across most of the bar's travel instead of
/// leaving the waveform permanently flat.
const RMS_FULL_SCALE: f32 = 0.25;

/// Smooths and rate-limits raw PCM buffers into a display level.
pub struct LevelMeter {
    level: f32,
    last_publish: Option<Instant>,
}

impl LevelMeter {
    pub fn new() -> Self {
        Self { level: 0.0, last_publish: None }
    }

    /// Folds one callback buffer into the current level. Returns `Some(level)` when
    /// enough time has passed to publish, otherwise `None` — the level keeps
    /// accumulating either way, so skipped buffers still influence the next value.
    pub fn push(&mut self, samples: &[f32], now: Instant) -> Option<f32> {
        self.level = smooth(self.level, normalized_rms(samples));

        let due = match self.last_publish {
            None => true,
            Some(last) => now.duration_since(last) >= PUBLISH_INTERVAL,
        };
        if due {
            self.last_publish = Some(now);
            Some(self.level)
        } else {
            None
        }
    }
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// Root-mean-square amplitude of a buffer, scaled so ordinary speech spans most of
/// 0..=1 and clamped so a loud transient can't overshoot.
fn normalized_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    (rms / RMS_FULL_SCALE).clamp(0.0, 1.0)
}

/// Moves `current` toward `target`, rising faster than it falls.
fn smooth(current: f32, target: f32) -> f32 {
    let coeff = if target > current { ATTACK } else { RELEASE };
    let next = current + (target - current) * coeff;
    next.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_reads_zero() {
        assert_eq!(normalized_rms(&[0.0; 512]), 0.0);
    }

    #[test]
    fn an_empty_buffer_reads_zero_rather_than_nan() {
        // len 0 would divide by zero; guard must come first.
        let v = normalized_rms(&[]);
        assert!(v.is_finite());
        assert_eq!(v, 0.0);
    }

    #[test]
    fn full_scale_audio_saturates_at_one() {
        // Alternating +/-1 has RMS 1.0, far above RMS_FULL_SCALE.
        let buf: Vec<f32> = (0..512).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        assert_eq!(normalized_rms(&buf), 1.0);
    }

    #[test]
    fn speech_level_audio_lands_mid_range() {
        // RMS 0.1 is typical speech; should be clearly audible on the bars but not pinned.
        let buf = vec![0.1f32; 512];
        let v = normalized_rms(&buf);
        assert!(v > 0.3 && v < 0.5, "speech-level rms mapped to {v}");
    }

    #[test]
    fn rms_is_sign_independent() {
        let positive = vec![0.2f32; 256];
        let negative = vec![-0.2f32; 256];
        assert_eq!(normalized_rms(&positive), normalized_rms(&negative));
    }

    #[test]
    fn output_is_always_within_zero_and_one() {
        // Values beyond nominal range (clipping, bad format conversion) must not escape.
        for buf in [vec![9.0f32; 64], vec![-9.0f32; 64], vec![f32::MAX; 64]] {
            let v = normalized_rms(&buf);
            assert!((0.0..=1.0).contains(&v), "rms escaped range: {v}");
        }
    }

    #[test]
    fn smoothing_rises_toward_the_target_without_overshooting() {
        let mut level = 0.0;
        for _ in 0..40 {
            let next = smooth(level, 1.0);
            assert!(next >= level, "level went down while rising");
            assert!(next <= 1.0, "level overshot: {next}");
            level = next;
        }
        assert!(level > 0.99, "never converged, stalled at {level}");
    }

    #[test]
    fn smoothing_falls_back_to_silence() {
        let mut level = 1.0;
        for _ in 0..80 {
            let next = smooth(level, 0.0);
            assert!(next <= level, "level went up while falling");
            assert!(next >= 0.0);
            level = next;
        }
        assert!(level < 0.01, "never decayed, stuck at {level}");
    }

    #[test]
    fn attack_is_faster_than_release() {
        // Speech onset should register faster than it decays.
        let rise = smooth(0.5, 1.0) - 0.5;
        let fall = 0.5 - smooth(0.5, 0.0);
        assert!(rise > fall, "attack {rise} not faster than release {fall}");
    }

    #[test]
    fn first_push_publishes_immediately() {
        let mut meter = LevelMeter::new();
        assert!(meter.push(&[0.2; 128], Instant::now()).is_some());
    }

    #[test]
    fn pushes_within_the_interval_are_withheld() {
        let mut meter = LevelMeter::new();
        let t0 = Instant::now();
        assert!(meter.push(&[0.2; 128], t0).is_some());
        assert!(meter.push(&[0.2; 128], t0 + Duration::from_millis(5)).is_none());
        assert!(meter.push(&[0.2; 128], t0 + Duration::from_millis(20)).is_none());
    }

    #[test]
    fn a_push_after_the_interval_publishes_again() {
        let mut meter = LevelMeter::new();
        let t0 = Instant::now();
        meter.push(&[0.2; 128], t0);
        assert!(meter.push(&[0.2; 128], t0 + PUBLISH_INTERVAL).is_some());
    }

    #[test]
    fn withheld_buffers_still_move_the_level() {
        // Loud audio arriving between publishes must not be discarded, or the
        // waveform would lag behind the voice.
        let mut meter = LevelMeter::new();
        let t0 = Instant::now();
        meter.push(&[0.0; 128], t0); // publishes ~0
        for i in 1..5 {
            meter.push(&[0.3; 128], t0 + Duration::from_millis(i * 5));
        }
        let published = meter
            .push(&[0.3; 128], t0 + PUBLISH_INTERVAL)
            .expect("should publish after interval");
        assert!(published > 0.5, "quiet buffers were dropped, got {published}");
    }

    #[test]
    fn published_levels_stay_in_range() {
        let mut meter = LevelMeter::new();
        let t0 = Instant::now();
        for i in 0..50 {
            let buf = if i % 2 == 0 { vec![5.0f32; 64] } else { vec![0.0f32; 64] };
            if let Some(v) = meter.push(&buf, t0 + PUBLISH_INTERVAL * i) {
                assert!((0.0..=1.0).contains(&v), "published {v}");
            }
        }
    }
}
