use super::{Shared, State, decode::AudioDecoder, output::AudioSource, tempo::Tempo};
use crate::{
    gateway::Connection,
    preview::{Source, Target},
};
use std::{
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};
use symphonia::core::io::MediaSource;
struct Remote(crate::gateway::stream::range::RangeSource);
impl std::io::Read for Remote {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, out)
    }
}
impl std::io::Seek for Remote {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.0, to)
    }
}
impl MediaSource for Remote {
    fn is_seekable(&self) -> bool {
        self.0.seekable()
    }
    fn byte_len(&self) -> Option<u64> {
        self.0.byte_len()
    }
}
pub(super) fn run(connection: Connection, target: Target, shared: Arc<Shared>) {
    while !shared.cancel.cancelled() {
        let control = shared.controls.lock().unwrap();
        let generation = control.generation;
        let source_cancel = crate::gateway::stream::Cancel::default();
        *shared.source_cancel.lock().unwrap() = Some(source_cancel.clone());
        drop(control);
        shared.playback_update(generation, |s| {
            s.state = State::Loading;
            s.error = None;
        });
        // Untrusted decoder panics must become a failed preview, not a permanently loading worker.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let source: Box<dyn MediaSource> = match &target.source {
                Source::AgentPath(path) => {
                    Box::new(Remote(crate::gateway::stream::range::RangeSource::open(
                        &connection,
                        &target.owner.profile,
                        path,
                        target.owner.session.clone(),
                        source_cancel.clone(),
                    )?))
                }
                Source::Inline { bytes, .. } => Box::new(Cursor::new(bytes.clone())),
                Source::RemoteUrl(url) => Box::new(Remote(
                    crate::gateway::stream::range::RangeSource::open_external(
                        url,
                        source_cancel.clone(),
                    )?,
                )),
            };
            let decoder = AudioDecoder::open(source, &target.label)?;
            play(decoder, &shared, generation)
        }))
        .unwrap_or_else(|_| Err("The audio decoder rejected malformed media".into()));
        if shared.cancel.cancelled() {
            break;
        }
        let mut control = shared.controls.lock().unwrap();
        if control.generation != generation {
            continue;
        }
        if let Err(error) = result {
            shared.update(|s| {
                s.state = State::Error;
                s.paused = true;
                s.error = Some(error);
            });
        }
        while !shared.cancel.cancelled() && control.generation == generation {
            control = shared.wake.wait(control).unwrap();
        }
    }
}
fn play(mut decoder: AudioDecoder, shared: &Arc<Shared>, generation: u64) -> Result<(), String> {
    let (rate, channels) = (decoder.rate, decoder.channels);
    let control = shared.controls.lock().unwrap().clone();
    if control.generation != generation {
        return Ok(());
    }
    let position = control.seek.unwrap_or(0.);
    if position > 0. {
        decoder.seek(position)?;
    }
    let mut tempo = Tempo::new(rate, channels, control.rate)?;
    {
        let current = shared.controls.lock().unwrap();
        if current.generation != generation {
            return Ok(());
        }
        shared.ring.lock().unwrap().reset(position, rate, channels);
    }
    shared.playback_update(generation, |s| {
        s.duration = decoder.duration;
        s.seekable = decoder.seekable;
        s.position = position;
        s.paused = control.paused;
        s.rate = control.rate;
        s.volume = control.volume;
        s.muted = control.muted;
        s.state = if control.paused {
            State::Paused
        } else {
            State::Buffering
        };
    });
    let mut output: Option<rodio::OutputStream> = None;
    let mut sink: Option<rodio::Sink> = None;
    let mut previous = control;
    let mut ended = false;
    let mut drained = false;
    let mut progress = Instant::now();
    while !shared.cancel.cancelled() {
        let control = shared.controls.lock().unwrap().clone();
        if control.generation != generation {
            return Ok(());
        }
        if control.rate != previous.rate {
            tempo.set_speed(control.rate);
        }
        if control.volume != previous.volume || control.muted != previous.muted {
            if let Some(sink) = &sink {
                sink.set_volume(if control.muted { 0. } else { control.volume });
            }
        }
        if control.rate != previous.rate
            || control.volume != previous.volume
            || control.muted != previous.muted
        {
            shared.playback_update(generation, |s| {
                s.rate = control.rate;
                s.volume = control.volume;
                s.muted = control.muted;
            });
        }
        if control.paused {
            if let Some(sink) = &sink {
                sink.pause();
            }
            shared.ring.lock().unwrap().paused = true;
            if !ended && previous.paused != control.paused {
                shared.playback_update(generation, |s| s.state = State::Paused);
            }
            previous = control;
            let c = shared.controls.lock().unwrap();
            if c.generation == generation
                && c.revision == previous.revision
                && c.paused
                && !shared.cancel.cancelled()
            {
                drop(shared.wake.wait(c).unwrap());
            }
            continue;
        }
        if ended {
            if decoder.seekable {
                decoder.seek(0.)?;
                tempo = Tempo::new(rate, channels, control.rate)?;
                shared.ring.lock().unwrap().reset(0., rate, channels);
                ended = false;
                drained = false;
            } else {
                return Err("This stream has ended. Reload it to play again.".into());
            }
        }
        if output.is_none() {
            let stream = rodio::OutputStreamBuilder::open_default_stream()
                .map_err(|_| "No audio output is available. Choose speakers in system settings.")?;
            let new_sink = rodio::Sink::connect_new(stream.mixer());
            new_sink.pause();
            new_sink.set_volume(if control.muted { 0. } else { control.volume });
            new_sink.append(AudioSource {
                shared: shared.clone(),
                rate,
                channels,
            });
            sink = Some(new_sink);
            output = Some(stream);
        }
        let queued = shared.ring.lock().unwrap().queued;
        if queued < rate as usize * channels as usize / 2 && !drained {
            let mut emitted = tempo.read();
            if emitted.is_empty() {
                match decoder.next()? {
                    Some(samples) => {
                        tempo.write(&samples)?;
                        emitted = tempo.read();
                    }
                    None => {
                        tempo.finish()?;
                        emitted = tempo.read();
                        if emitted.is_empty() {
                            drained = true;
                        }
                    }
                }
            }
            let current = shared.controls.lock().unwrap();
            if current.generation != generation {
                return Ok(());
            }
            let mut ring = shared.ring.lock().unwrap();
            if !emitted.is_empty() {
                ring.push(emitted, control.rate);
            }
            if drained {
                ring.eof = true;
            }
        }
        let mut current = shared.controls.lock().unwrap();
        if current.generation != generation {
            return Ok(());
        }
        let mut ring = shared.ring.lock().unwrap();
        ring.paused = current.paused;
        let waiting = ring.queued == 0 && !ring.eof;
        if ring.eof && ring.queued == 0 {
            ended = true;
            ring.paused = true;
            ring.position = decoder.duration.unwrap_or(ring.position);
            current.paused = true;
        }
        let position = ring.position;
        drop(ring);
        let paused = current.paused;
        drop(current);
        if let Some(sink) = &sink {
            if paused {
                sink.pause();
            } else {
                sink.play();
            }
        }
        if progress.elapsed() >= Duration::from_millis(50) || ended || previous.paused || paused {
            shared.playback_progress(generation, |s| {
                s.position = position.min(s.duration.unwrap_or(f64::MAX));
                s.paused = paused;
                s.state = if ended {
                    State::Ended
                } else if paused {
                    State::Paused
                } else if waiting {
                    State::Buffering
                } else {
                    State::Playing
                };
            });
            progress = Instant::now();
        }
        previous = control;
        if queued >= rate as usize * channels as usize / 2 || drained {
            let c = shared.controls.lock().unwrap();
            if c.generation == generation {
                let _ = shared
                    .wake
                    .wait_timeout(c, Duration::from_millis(10))
                    .unwrap();
            }
        }
    }
    Ok(())
}
