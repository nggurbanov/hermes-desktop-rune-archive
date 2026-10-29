//! Per-operation IO budgets bound aggregate metadata, independently of source size.
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use symphonia::core::io::MediaSource;
#[derive(Clone)]
pub(super) struct Budget {
    bytes: Arc<AtomicU64>,
    calls: Arc<AtomicU64>,
    magic: Arc<Mutex<([u8; 4], u8)>>,
}
impl Budget {
    pub fn reset(&self, bytes: u64) {
        self.bytes.store(bytes, Ordering::Release);
        self.calls.store(65536, Ordering::Release);
    }
    pub fn is_ogg(&self) -> bool {
        let (magic, mask) = *self.magic.lock().unwrap();
        mask == 15 && magic == *b"OggS"
    }
}
struct Source {
    inner: Box<dyn MediaSource>,
    budget: Budget,
    position: u64,
}
pub(super) fn wrap(inner: Box<dyn MediaSource>) -> (Box<dyn MediaSource>, Budget) {
    let budget = Budget {
        bytes: Arc::new(AtomicU64::new(64 * 1024 * 1024)),
        calls: Arc::new(AtomicU64::new(65536)),
        magic: Arc::new(Mutex::new(([0; 4], 0))),
    };
    (
        Box::new(Source {
            inner,
            budget: budget.clone(),
            position: 0,
        }),
        budget,
    )
}
impl Read for Source {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self
            .budget
            .calls
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Audio operation exceeded its IO-call budget",
            ));
        }
        let left = self.budget.bytes.load(Ordering::Acquire);
        if left == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Audio operation exceeded its read budget",
            ));
        }
        let limit = out.len().min(left as usize);
        let count = self.inner.read(&mut out[..limit])?;
        self.budget.bytes.fetch_sub(count as u64, Ordering::AcqRel);
        if self.position < 4 && count > 0 {
            let mut magic = self.budget.magic.lock().unwrap();
            for (i, byte) in out[..count.min(4 - self.position as usize)]
                .iter()
                .enumerate()
            {
                let pos = self.position as usize + i;
                magic.0[pos] = *byte;
                magic.1 |= 1 << pos;
            }
        }
        self.position = self.position.saturating_add(count as u64);
        Ok(count)
    }
}
impl Seek for Source {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.position = self.inner.seek(to)?;
        Ok(self.position)
    }
}
impl MediaSource for Source {
    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }
    fn byte_len(&self) -> Option<u64> {
        self.inner.byte_len()
    }
}
