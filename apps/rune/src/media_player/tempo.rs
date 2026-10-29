//! The unmodified, statically linked Sonic processor owns pitch-preserving tempo.
use std::{
    ffi::{c_float, c_int, c_void},
    ptr::NonNull,
};
unsafe extern "C" {
    fn sonicCreateStream(rate: c_int, channels: c_int) -> *mut c_void;
    fn sonicDestroyStream(stream: *mut c_void);
    fn sonicSetSpeed(stream: *mut c_void, speed: c_float);
    fn sonicSetPitch(stream: *mut c_void, pitch: c_float);
    fn sonicSetRate(stream: *mut c_void, rate: c_float);
    fn sonicWriteFloatToStream(
        stream: *mut c_void,
        samples: *const c_float,
        frames: c_int,
    ) -> c_int;
    fn sonicReadFloatFromStream(stream: *mut c_void, samples: *mut c_float, frames: c_int)
    -> c_int;
    fn sonicFlushStream(stream: *mut c_void) -> c_int;
}
pub(super) struct Tempo {
    stream: NonNull<c_void>,
    channels: usize,
    flushed: bool,
}
impl Tempo {
    pub fn new(rate: u32, channels: u16, speed: f32) -> Result<Self, String> {
        if !(8000..=192000).contains(&rate)
            || !(1..=8).contains(&channels)
            || !speed.is_finite()
            || !(0.25..=4.).contains(&speed)
        {
            return Err("Unsupported audio format or playback rate".into());
        }
        let stream = NonNull::new(unsafe { sonicCreateStream(rate as c_int, channels as c_int) })
            .ok_or("The tempo processor could not allocate its stream")?;
        unsafe {
            sonicSetPitch(stream.as_ptr(), 1.);
            sonicSetRate(stream.as_ptr(), 1.);
            sonicSetSpeed(stream.as_ptr(), speed);
        }
        Ok(Self {
            stream,
            channels: channels as usize,
            flushed: false,
        })
    }
    pub fn write(&mut self, samples: &[f32]) -> Result<(), String> {
        if self.flushed
            || samples.len() % self.channels != 0
            || samples.len() / self.channels > 1024
        {
            return Err("Invalid tempo input batch".into());
        }
        if unsafe {
            sonicWriteFloatToStream(
                self.stream.as_ptr(),
                samples.as_ptr(),
                (samples.len() / self.channels) as c_int,
            )
        } == 0
        {
            return Err("The tempo processor ran out of memory".into());
        }
        Ok(())
    }
    pub fn set_speed(&mut self, speed: f32) {
        unsafe {
            sonicSetSpeed(self.stream.as_ptr(), speed);
        }
    }
    pub fn read(&mut self) -> Vec<f32> {
        let mut out = vec![0.; 4096 * self.channels];
        let frames =
            unsafe { sonicReadFloatFromStream(self.stream.as_ptr(), out.as_mut_ptr(), 4096) };
        out.truncate(frames.max(0) as usize * self.channels);
        out
    }
    pub fn finish(&mut self) -> Result<(), String> {
        if !self.flushed {
            if unsafe { sonicFlushStream(self.stream.as_ptr()) } == 0 {
                return Err("The tempo processor could not finish the stream".into());
            }
            self.flushed = true;
        }
        Ok(())
    }
}
impl Drop for Tempo {
    fn drop(&mut self) {
        unsafe { sonicDestroyStream(self.stream.as_ptr()) }
    }
}
