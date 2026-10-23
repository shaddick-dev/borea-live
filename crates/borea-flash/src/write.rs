//! Copy the image onto the target, flush it to the device, and read it back to prove
//! it arrived intact.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::FlashError;

const CHUNK: usize = 4 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Writing,
    /// Waiting for the device to confirm everything is on it. Can take a while on
    /// slow sticks, with no byte count to show.
    Syncing,
    Verifying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Every byte was read back from the device and matches the image.
    Verified,
    /// Stopped by the user. The stick holds a partial image and will not boot.
    Cancelled(Phase),
}

/// Where the image goes: a block device in use, a plain file in tests.
pub trait Sink: Read + Write + Seek {
    /// Make the data durable on the device itself.
    fn sync(&mut self) -> io::Result<()>;
    /// Drop cached copies so that reading back reads the device, not memory.
    fn forget_cache(&mut self) -> io::Result<()>;
}

impl Sink for File {
    fn sync(&mut self) -> io::Result<()> {
        self.sync_all()
    }

    fn forget_cache(&mut self) -> io::Result<()> {
        rustix::fs::fadvise(&*self, 0, None, rustix::fs::Advice::DontNeed).map_err(io::Error::from)
    }
}

pub fn flash(
    image: &mut (impl Read + Seek),
    size: u64,
    target: &mut impl Sink,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<Outcome, FlashError> {
    let mut buf = vec![0u8; CHUNK];
    let report = |phase, done| Progress { phase, done, total: size };

    image.seek(SeekFrom::Start(0)).map_err(FlashError::Read)?;
    target.seek(SeekFrom::Start(0)).map_err(FlashError::Write)?;
    let mut done = 0;
    while done < size {
        if cancel.load(Ordering::Relaxed) {
            let _ = target.sync();
            return Ok(Outcome::Cancelled(Phase::Writing));
        }
        let n = read_chunk(image, &mut buf, size - done).map_err(FlashError::Read)?;
        target.write_all(&buf[..n]).map_err(FlashError::Write)?;
        done += n as u64;
        progress(report(Phase::Writing, done));
    }

    progress(report(Phase::Syncing, size));
    target.flush().map_err(FlashError::Write)?;
    target.sync().map_err(FlashError::Write)?;
    target.forget_cache().map_err(FlashError::Write)?;

    image.seek(SeekFrom::Start(0)).map_err(FlashError::Read)?;
    target.seek(SeekFrom::Start(0)).map_err(FlashError::Write)?;
    let mut back = vec![0u8; CHUNK];
    let mut checked = 0;
    while checked < size {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled(Phase::Verifying));
        }
        let n = read_chunk(image, &mut buf, size - checked).map_err(FlashError::Read)?;
        target.read_exact(&mut back[..n]).map_err(FlashError::Write)?;
        if let Some(i) = buf[..n].iter().zip(&back[..n]).position(|(a, b)| a != b) {
            return Err(FlashError::Mismatch(checked + i as u64));
        }
        checked += n as u64;
        progress(report(Phase::Verifying, checked));
    }
    Ok(Outcome::Verified)
}

/// Fill as much of `buf` as `remaining` allows; short reads are retried.
fn read_chunk(image: &mut impl Read, buf: &mut [u8], remaining: u64) -> io::Result<usize> {
    let want = buf.len().min(remaining as usize);
    image.read_exact(&mut buf[..want])?;
    Ok(want)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::tests::{fake_iso, temp_dir};
    use std::io::Cursor;

    /// A target that corrupts one byte on its way to "disk".
    struct Flaky(Cursor<Vec<u8>>, u64);

    impl Read for Flaky {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }
    impl Write for Flaky {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let at = self.0.position();
            let n = self.0.write(buf)?;
            if (at..at + n as u64).contains(&self.1) {
                self.0.get_mut()[self.1 as usize] ^= 0xff;
            }
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Seek for Flaky {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.0.seek(pos)
        }
    }
    impl Sink for Flaky {
        fn sync(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn forget_cache(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn writes_and_verifies_onto_a_device_sized_file() {
        let dir = temp_dir("write");
        let iso = fake_iso(&dir, "borea-live.iso", "BOREA", 9 << 20);
        let stick = dir.join("stick.img");
        std::fs::write(&stick, vec![0xEE; 16 << 20]).unwrap();

        let mut image = File::open(&iso).unwrap();
        let mut target = File::options().read(true).write(true).open(&stick).unwrap();
        let mut phases = Vec::new();
        let outcome = flash(&mut image, 9 << 20, &mut target, &AtomicBool::new(false), |p| {
            if phases.last() != Some(&p.phase) {
                phases.push(p.phase);
            }
        })
        .unwrap();

        assert_eq!(outcome, Outcome::Verified);
        assert_eq!(phases, [Phase::Writing, Phase::Syncing, Phase::Verifying]);
        let written = std::fs::read(&stick).unwrap();
        assert_eq!(&written[..9 << 20], std::fs::read(&iso).unwrap().as_slice());
        assert!(written[9 << 20..].iter().all(|b| *b == 0xEE), "nothing past the image is touched");
    }

    #[test]
    fn a_corrupted_byte_is_caught() {
        let dir = temp_dir("corrupt");
        let iso = fake_iso(&dir, "borea-live.iso", "BOREA", 6 << 20);
        let mut image = File::open(&iso).unwrap();
        let mut target = Flaky(Cursor::new(vec![0; 8 << 20]), 5_000_123);
        let result = flash(&mut image, 6 << 20, &mut target, &AtomicBool::new(false), |_| {});
        assert!(matches!(result, Err(FlashError::Mismatch(5_000_123))), "{result:?}");
    }

    #[test]
    fn cancelling_stops_before_the_next_chunk() {
        let dir = temp_dir("cancel");
        let iso = fake_iso(&dir, "borea-live.iso", "BOREA", 12 << 20);
        let mut image = File::open(&iso).unwrap();
        let mut target = Flaky(Cursor::new(vec![0; 16 << 20]), u64::MAX);
        let cancel = AtomicBool::new(false);
        let mut chunks = 0;
        let outcome = flash(&mut image, 12 << 20, &mut target, &cancel, |p| {
            chunks += 1;
            if p.phase == Phase::Writing {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert_eq!(outcome, Outcome::Cancelled(Phase::Writing));
        assert_eq!(chunks, 1);
    }
}
