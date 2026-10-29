use super::*;

fn energy(samples: &[f32]) -> f64 {
    samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / samples.len() as f64
}
fn speech(sample: usize, rate: f32, frequency: f32) -> f32 {
    let t = sample as f32 / rate;
    let envelope = (0.5 + 0.5 * (t * 2.8).sin()).powi(2).min(1.0);
    envelope
        * ((t * frequency * std::f32::consts::TAU + 11.0 * (t * 0.7).sin()).sin() * 0.2
            + (t * frequency * 2.3 * std::f32::consts::TAU).sin() * 0.05)
}
#[test]
fn aec3_suppresses_rendered_echo_and_retains_near_end_during_duplex() {
    let dsp = VoiceDsp::new(16000).unwrap();
    let mut reference = RenderReference::new(dsp.clone());
    let mut far = Vec::new();
    let mut original = Vec::new();
    let mut residual = Vec::new();
    let mut near_original = Vec::new();
    let mut near_processed = Vec::new();
    // Feed the production renderer and capture adapters for 16 seconds. The
    // simulated room returns a 40ms delayed echo while near-end speech starts
    // after adaptation; the far-end never stops during the double-talk phase.
    for tick in 0..1600 {
        let rendered: Vec<f32> = (0..160)
            .map(|i| speech(tick * 160 + i, 16000.0, 191.0))
            .collect();
        far.extend_from_slice(&rendered);
        for sample in &rendered {
            for _ in 0..3 {
                reference.sample(*sample).unwrap();
                reference.sample(*sample).unwrap();
            }
        }
        let mut input = Vec::with_capacity(160);
        let mut near = Vec::with_capacity(160);
        for i in 0..160 {
            let position = tick * 160 + i;
            let echo = position
                .checked_sub(640)
                .map(|index| far[index] * 0.45)
                .unwrap_or(0.0);
            let local = if tick >= 1000 {
                speech(position, 16000.0, 347.0) * 0.7
            } else {
                0.0
            };
            input.push(echo + local);
            near.push(local);
        }
        let mut pcm: Vec<i16> = input.iter().map(|x| (x * 32767.0) as i16).collect();
        dsp.capture(&mut pcm).unwrap();
        let processed: Vec<f32> = pcm.iter().map(|x| f32::from(*x) / 32768.0).collect();
        if (600..1000).contains(&tick) {
            original.extend(input);
            residual.extend_from_slice(&processed);
        }
        if tick >= 1200 {
            near_original.extend(near);
            near_processed.extend(processed);
        }
    }
    let reduction = 10.0 * (energy(&original) / energy(&residual).max(1e-12)).log10();
    assert!(
        reduction > 8.0,
        "AEC3 echo reduction was only {reduction:.2} dB"
    );
    // A blanket microphone mute or half-duplex speaker gate would pass the
    // echo check but fail this independent near-end retention check.
    assert!(
        energy(&near_processed) > energy(&near_original) * 0.1,
        "Near-end speech was suppressed during simultaneous playback"
    );
    // The filterbank contributes a small processing delay. Compare against
    // the delayed near-end reference, rather than treating that latency as
    // lost speech; no unconstrained alignment or scale fitting is allowed.
    let correlation = (0..480)
        .map(|delay| {
            let actual = &near_processed[delay..];
            let expected = &near_original[..actual.len()];
            let dot: f64 = actual
                .iter()
                .zip(expected)
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum();
            dot / ((energy(actual) * energy(expected)).sqrt() * actual.len() as f64)
        })
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        correlation > 0.6,
        "Near-end speech correlation was {correlation:.3}"
    );
    eprintln!(
        "AEC3 echo attenuation {reduction:.2}dB, duplex near-end correlation {correlation:.3}"
    );
}

#[test]
fn processing_requires_complete_frames_and_resampling_preserves_timing() {
    assert!(VoiceDsp::new(11025).is_err());
    for rate in [8000, 16000] {
        let dsp = VoiceDsp::new(rate).unwrap();
        assert!(dsp.capture(&mut [0; 1]).is_err());
        let mut reference = RenderReference::new(dsp.clone());
        for sample in 0..960 {
            reference
                .sample(if sample % 2 == 0 { 0.6 } else { 0.2 })
                .unwrap();
        }
        assert_eq!(reference.phase, 0);
        assert!(reference.frame.is_empty());
        dsp.capture(&mut vec![0; (rate / 100) as usize]).unwrap();
    }
}
