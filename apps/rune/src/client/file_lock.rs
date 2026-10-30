//! A logical lock ends with its owner, even if fork/dup retained the file description.
use std::fs::File;

#[derive(Debug)]
pub(crate) struct FileLock(File);
impl FileLock {
    /// Takes ownership after the caller has successfully acquired the file lock.
    pub(crate) fn from_locked(file: File) -> Self {
        Self(file)
    }
    /// Models a forked child's inherited descriptor without a timing-dependent fork.
    #[cfg(test)]
    pub(crate) fn try_clone(&self) -> std::io::Result<File> {
        self.0.try_clone()
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing only this File leaves flock held by any duplicated/inherited
        // descriptor. Unlock explicitly at the final logical owner's boundary.
        if let Err(error) = self.0.unlock() {
            eprintln!("Desktop file lock release failed: {error}");
        }
    }
}
