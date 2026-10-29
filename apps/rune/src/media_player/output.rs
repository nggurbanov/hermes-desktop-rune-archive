use super::Shared;
use std::{collections::VecDeque, sync::Arc, time::Duration};
struct Chunk {
    samples: Vec<f32>,
    offset: usize,
    step: f64,
}
pub(super) struct Ring {
    chunks: VecDeque<Chunk>,
    pub queued: usize,
    pub position: f64,
    pub closed: bool,
    pub paused: bool,
    pub eof: bool,
    pub rate: u32,
    pub channels: u16,
    pub underrun: bool,
}
impl Ring {
    pub fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            queued: 0,
            position: 0.,
            closed: false,
            paused: true,
            eof: false,
            rate: 48000,
            channels: 2,
            underrun: false,
        }
    }
    pub fn reset(&mut self, position: f64, rate: u32, channels: u16) {
        self.chunks.clear();
        self.queued = 0;
        self.position = position;
        self.rate = rate;
        self.channels = channels;
        self.eof = false;
        self.underrun = false;
    }
    pub fn push(&mut self, samples: Vec<f32>, speed: f32) {
        self.queued += samples.len();
        self.chunks.push_back(Chunk {
            samples,
            offset: 0,
            step: speed as f64 / (self.rate as f64 * self.channels as f64),
        });
    }
    fn sample(&mut self) -> Option<f32> {
        if self.closed {
            return None;
        }
        if self.paused {
            return Some(0.);
        }
        let Some(chunk) = self.chunks.front_mut() else {
            self.underrun = !self.eof;
            return Some(0.);
        };
        let value = chunk.samples[chunk.offset];
        chunk.offset += 1;
        self.queued -= 1;
        self.position += chunk.step;
        if chunk.offset == chunk.samples.len() {
            self.chunks.pop_front();
        }
        self.underrun = false;
        Some(value)
    }
}
pub(super) struct AudioSource {
    pub shared: Arc<Shared>,
    pub rate: u32,
    pub channels: u16,
}
impl Iterator for AudioSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.shared.ring.lock().unwrap().sample()
    }
}
impl rodio::Source for AudioSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
#[cfg(test)]
pub(super) fn consume(ring: &mut Ring, count: usize) -> Vec<f32> {
    (0..count).filter_map(|_| ring.sample()).collect()
}
