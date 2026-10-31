use std::collections::VecDeque;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, CODEC_TYPE_OPUS, Decoder, DecoderOptions},
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
    units::{Time, TimeBase},
};
pub(super) struct AudioDecoder {
    format: Box<dyn FormatReader>,
    codec: Codec,
    track: u32,
    time_base: TimeBase,
    pub rate: u32,
    pub channels: u16,
    pub duration: Option<f64>,
    pub seekable: bool,
    pending: VecDeque<f32>,
    discard_before: f64,
    last_end: f64,
    budget: super::budget::Budget,
    #[cfg(test)]
    diagnostic: Option<(std::sync::Weak<super::Shared>, u64)>,
}
enum Codec {
    Native(Box<dyn Decoder>),
    Opus {
        decoder: opus::Decoder,
        gain: f32,
        pre_skip: u32,
    },
}
impl AudioDecoder {
    pub fn open(source: Box<dyn MediaSource>, label: &str) -> Result<Self, String> {
        let (source, budget) = super::budget::wrap(source);
        let seekable = source.is_seekable();
        let mut hint = Hint::new();
        if let Some(ext) = label.rsplit('.').next() {
            hint.with_extension(ext);
        }
        let stream = MediaSourceStream::new(source, Default::default());
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
                &MetadataOptions {
                    limit_metadata_bytes: symphonia::core::meta::Limit::Maximum(4 * 1024 * 1024),
                    limit_visual_bytes: symphonia::core::meta::Limit::Maximum(1024 * 1024),
                },
            )
            .map_err(|_| "This audio container could not be read")?;
        let mut format = probed.format;
        let track = format
            .default_track()
            .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .or_else(|| {
                format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            })
            .ok_or("The file contains no audio track")?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        if params.codec == CODEC_TYPE_OPUS && !budget.is_ogg() {
            return Err("Opus playback currently supports Ogg containers only".into());
        }
        if params
            .sample_rate
            .is_some_and(|v| !(8000..=192000).contains(&v))
            || params
                .channels
                .is_some_and(|v| !(1..=8).contains(&v.count()))
            || params.bits_per_sample.is_some_and(|v| v > 64)
            || params.max_frames_per_packet.is_some_and(|v| v > 384000)
        {
            return Err("Audio codec parameters exceed allocation limits".into());
        }
        let mut first = VecDeque::new();
        let mut first_end = 0.;
        let mut native = if params.codec != CODEC_TYPE_OPUS {
            Some(
                symphonia::default::get_codecs()
                    .make(&params, &DecoderOptions::default())
                    .map_err(|_| "This audio codec is unsupported")?,
            )
        } else {
            None
        };
        let mut discovered = None;
        if (params.sample_rate.is_none() || params.channels.is_none())
            && let Some(decoder) = native.as_mut()
        {
            for _ in 0..1024 {
                let packet = format
                    .next_packet()
                    .map_err(|_| "The audio metadata could not be decoded")?;
                if packet.track_id() != track_id {
                    continue;
                }
                let decoded = decoder
                    .decode(&packet)
                    .map_err(|_| "The audio metadata could not be decoded")?;
                let spec = *decoded.spec();
                if !(8000..=192000).contains(&spec.rate)
                    || !(1..=8).contains(&spec.channels.count())
                    || decoded.frames() > spec.rate as usize * 2
                {
                    return Err("Unsupported audio metadata format".into());
                }
                let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buffer.copy_interleaved_ref(decoded);
                if buffer.samples().iter().any(|v| !v.is_finite()) {
                    return Err("The audio contains invalid floating-point samples".into());
                }
                first.extend(buffer.samples().iter().map(|v| v.clamp(-1., 1.)));
                first_end = buffer.samples().len() as f64
                    / (spec.rate as f64 * spec.channels.count() as f64);
                discovered = Some(spec);
                break;
            }
        }
        let rate = params
            .sample_rate
            .or_else(|| discovered.map(|s| s.rate))
            .ok_or("The audio sample rate is missing")?;
        let channels = params
            .channels
            .or_else(|| discovered.map(|s| s.channels))
            .ok_or("The audio channel layout is missing")?
            .count() as u16;
        if !(8000..=192000).contains(&rate) || !(1..=8).contains(&channels) {
            return Err("This audio sample rate or channel count is unsupported".into());
        }
        let time_base = params.time_base.unwrap_or(TimeBase::new(1, rate));
        let mut duration = params
            .n_frames
            .map(|n| seconds(time_base.calc_time(n)))
            .filter(|t| t.is_finite() && *t >= 0.);
        let codec = if params.codec == CODEC_TYPE_OPUS {
            let header = params
                .extra_data
                .as_deref()
                .ok_or("Opus identification header is missing")?;
            if header.len() != 19
                || &header[..8] != b"OpusHead"
                || header[8] > 15
                || header[9] != channels as u8
                || header[18] != 0
                || channels > 2
            {
                return Err(
                    "Only mono/stereo mapping-family-0 Ogg Opus is currently supported".into(),
                );
            }
            let gain =
                10f32.powf(i16::from_le_bytes([header[16], header[17]]) as f32 / (256. * 20.));
            let decoder = opus::Decoder::new(
                48000,
                if channels == 1 {
                    opus::Channels::Mono
                } else {
                    opus::Channels::Stereo
                },
            )
            .map_err(|_| "The Opus decoder could not start")?;
            duration = params.n_frames.map(|n| {
                seconds(time_base.calc_time(
                    n.saturating_sub(u16::from_le_bytes([header[10], header[11]]) as u64),
                ))
            });
            Codec::Opus {
                decoder,
                gain,
                pre_skip: u16::from_le_bytes([header[10], header[11]]) as u32,
            }
        } else {
            Codec::Native(native.take().unwrap())
        };
        let track = track_id;
        Ok(Self {
            format,
            codec,
            track,
            time_base,
            rate,
            channels,
            duration,
            seekable,
            pending: first,
            discard_before: 0.,
            last_end: first_end,
            budget,
            #[cfg(test)]
            diagnostic: super::TEST_PLAYER.with(|slot| {
                let weak = slot.borrow().clone();
                weak.upgrade().map(|player| {
                    let generation = player.controls.lock().unwrap().generation;
                    (weak, generation)
                })
            }),
        })
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        if !self.seekable {
            return Err("This source does not provide reliable seeking".into());
        }
        self.budget.reset(64 * 1024 * 1024);
        let seconds = seconds.max(0.).min(self.duration.unwrap_or(f64::MAX));
        let (preroll, offset) = match self.codec {
            Codec::Opus { pre_skip, .. } => (0.08, pre_skip as f64 / 48000.),
            _ => (0., 0.),
        };
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from((seconds + offset - preroll).max(0.)),
                    track_id: Some(self.track),
                },
            )
            .map_err(|_| "The audio source could not seek to that position")?;
        match &mut self.codec {
            Codec::Native(d) => d.reset(),
            Codec::Opus { decoder, .. } => decoder
                .reset_state()
                .map_err(|_| "The Opus decoder could not reset")?,
        }
        self.pending.clear();
        self.discard_before = seconds;
        self.last_end = seconds;
        Ok(())
    }
    pub fn next(&mut self) -> Result<Option<Vec<f32>>, String> {
        self.budget.reset(32 * 1024 * 1024);
        loop {
            if !self.pending.is_empty() {
                return Ok(Some(
                    self.pending
                        .drain(..self.pending.len().min(1024 * self.channels as usize))
                        .collect(),
                ));
            }
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    if self
                        .duration
                        .is_some_and(|duration| duration > self.last_end + 2. / self.rate as f64)
                    {
                        return Err(
                            "The audio file is truncated before its declared duration".into()
                        );
                    }
                    return Ok(None);
                }
                Err(error) => {
                    #[cfg(test)]
                    if let Some((player, generation)) = &self.diagnostic
                        && let Some(player) = player.upgrade()
                        && player.controls.lock().unwrap().generation == *generation
                    {
                        let detail = format!("{error:?}");
                        *player.packet_error.lock().unwrap() =
                            Some((*generation, detail.chars().take(2048).collect()));
                    }
                    #[cfg(not(test))]
                    let _ = error;
                    return Err("The audio stream ended unexpectedly or could not be read".into());
                }
            };
            if packet.track_id() != self.track {
                continue;
            }
            if packet.data.is_empty() {
                return Err("The audio file is truncated or contains an empty coded packet".into());
            }
            if packet.data.len() > 16 * 1024 * 1024 {
                return Err("The audio packet exceeds the decode limit".into());
            }
            let mut samples = match &mut self.codec {
                Codec::Native(decoder) => {
                    let decoded = decoder
                        .decode(&packet)
                        .map_err(|_| "An audio packet could not be decoded")?;
                    if decoded.spec().rate != self.rate
                        || decoded.spec().channels.count() != self.channels as usize
                        || decoded.frames() > self.rate as usize * 2
                    {
                        return Err("The audio format changed or exceeds the packet limit".into());
                    }
                    let mut buffer =
                        SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                    buffer.copy_interleaved_ref(decoded);
                    buffer.samples().to_vec()
                }
                Codec::Opus {
                    decoder,
                    gain,
                    pre_skip,
                } => {
                    let mut samples = vec![0.; 5760 * self.channels as usize];
                    let frames = decoder
                        .decode_float(&packet.data, &mut samples, false)
                        .map_err(|_| "An Opus packet could not be decoded")?;
                    let start = (packet
                        .trim_start()
                        .max(pre_skip.saturating_sub(packet.ts().min(u32::MAX as u64) as u32))
                        as usize)
                        .min(frames);
                    let end = frames.saturating_sub(packet.trim_end() as usize).max(start);
                    samples = samples[start * self.channels as usize..end * self.channels as usize]
                        .to_vec();
                    for sample in &mut samples {
                        *sample *= *gain;
                    }
                    samples
                }
            };
            if samples.iter().any(|sample| !sample.is_finite()) {
                return Err("The audio contains invalid floating-point samples".into());
            }
            for sample in &mut samples {
                *sample = sample.clamp(-1., 1.);
            }
            let packet_start = match self.codec {
                Codec::Opus { pre_skip, .. } => seconds(
                    self.time_base
                        .calc_time(packet.ts().saturating_sub(pre_skip as u64)),
                ),
                _ => seconds(self.time_base.calc_time(packet.ts())),
            };
            self.last_end = self.last_end.max(
                packet_start + samples.len() as f64 / (self.rate as f64 * self.channels as f64),
            );
            let drop_frames = if packet_start < self.discard_before {
                ((self.discard_before - packet_start) * self.rate as f64).round() as usize
            } else {
                0
            };
            let skip = (drop_frames * self.channels as usize).min(samples.len());
            samples.drain(..skip);
            self.pending.extend(samples);
        }
    }
}
fn seconds(time: Time) -> f64 {
    time.seconds as f64 + time.frac
}
