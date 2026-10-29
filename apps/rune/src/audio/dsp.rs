//! AEC3 shares the actual rendered speaker reference with capture processing.
//! Both clocks deliver 10ms frames; WebRTC estimates the device/acoustic delay.
use std::sync::Arc;
use webrtc_audio_processing::{
    Processor,
    config::{AdaptiveDigital, Config, EchoCanceller, GainController, GainController2},
};

pub(super) struct VoiceDsp {
    processor: Processor,
    rate: u32,
}
impl VoiceDsp {
    pub fn new(rate: u32) -> Result<Arc<Self>, String> {
        if !matches!(rate, 8000 | 16000) {
            return Err("The microphone rate is not supported by voice processing".into());
        }
        let processor = Processor::new(rate)
            .map_err(|_| "The native acoustic echo canceller could not initialize")?;
        processor.set_config(Config {
            high_pass_filter: Some(Default::default()),
            echo_canceller: Some(EchoCanceller::Full {
                stream_delay_ms: None,
            }),
            noise_suppression: Some(Default::default()),
            gain_controller: Some(GainController::GainController2(GainController2 {
                // The OS device gain stays under the user's control.
                adaptive_digital: Some(AdaptiveDigital {
                    max_gain_db: 20.0,
                    initial_gain_db: 0.0,
                    ..Default::default()
                }),
                ..Default::default()
            })),
            ..Default::default()
        });
        Ok(Arc::new(Self { processor, rate }))
    }
    pub fn capture(&self, pcm: &mut [i16]) -> Result<(), String> {
        let count = self.processor.num_samples_per_frame();
        if pcm.is_empty() || pcm.len() % count != 0 {
            return Err("The microphone did not supply complete 10ms processing frames".into());
        }
        let mut frame = vec![0.0; count];
        for samples in pcm.chunks_exact_mut(count) {
            for (sample, output) in samples.iter().zip(&mut frame) {
                *output = f32::from(*sample) / 32768.0;
            }
            self.processor
                .process_capture_frame([frame.as_mut_slice()])
                .map_err(|_| "Native microphone echo processing failed")?;
            for (sample, output) in frame.iter().zip(samples) {
                *output = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            }
        }
        Ok(())
    }
    fn render(&self, pcm: &[f32]) -> Result<(), String> {
        self.processor
            .analyze_render_frame([pcm])
            .map_err(|_| "Native speaker echo-reference processing failed".into())
    }
}

/// The renderer calls this for every stereo sample actually requested by the
/// output mixer, including silence and underruns, rather than queued packets.
pub(super) struct RenderReference {
    dsp: Arc<VoiceDsp>,
    frame: Vec<f32>,
    phase: u32,
    sum: f32,
}
impl RenderReference {
    pub fn new(dsp: Arc<VoiceDsp>) -> Self {
        Self {
            frame: Vec::with_capacity((dsp.rate / 100) as usize),
            dsp,
            phase: 0,
            sum: 0.0,
        }
    }
    pub fn sample(&mut self, sample: f32) -> Result<(), String> {
        // 48k stereo -> capture-rate mono. Integer groups preserve both
        // channel averaging and timing across arbitrary output callbacks.
        self.sum += sample;
        self.phase += 1;
        let group = 96000 / self.dsp.rate;
        if self.phase == group {
            self.frame.push(self.sum / group as f32);
            self.sum = 0.0;
            self.phase = 0;
            if self.frame.len() == (self.dsp.rate / 100) as usize {
                self.dsp.render(&self.frame)?;
                self.frame.clear();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
