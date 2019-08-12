use std::io::{Read, Error as IoError, ErrorKind};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

/// Fuses reader so that if writer thread dies while holding armed fuse the reader will get `BrokenPipe` error.
pub fn fuse<R: Read>(reader: R) -> (FusedReader<R>, Fuse) {
    let reader_fuse = Arc::new(Mutex::new(()));
    let writer_fuse = reader_fuse.clone();

    (
        FusedReader {
            reader,
            fuse: reader_fuse,
        },
        Fuse(writer_fuse),
    )
}

/// Reader that will fail with `BrokenPipe` error if writing end thread dies while holding armed
/// fuse.
pub struct FusedReader<R: Read> {
    reader: R,
    fuse: Arc<Mutex<()>>,
}

impl<R: Read> FusedReader<R> {
    /// Checks if the fuse is armed.
    ///
    /// Returns true if fuse is armed and `ArmedFuse` not dropped.
    /// Fails with `BrokenPipe` error if armed fuse was dropped due to panic.
    pub fn is_fuse_armed(&self) -> Result<bool, IoError> {
        match self.fuse.try_lock() {
            Err(TryLockError::Poisoned(_)) => Err(IoError::new(ErrorKind::BrokenPipe, "writer end dropped due to panic")), // armed fuse got dropped due to panic
            Ok(_) => Ok(false), // fuse dropped, writer gone
            Err(TryLockError::WouldBlock) => Ok(true), // fuse still in place
        }
    }

    /// Returns inner reader.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: Read> Read for FusedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        // let it read to end before checking fuse
        dbg![self.reader.read(buf)].and_then(|bytes| if bytes == 0 {
            self.is_fuse_armed().map(|_| bytes)
        } else {
            Ok(bytes)
        })
    }
}

/// Armed fuse that if dropped due to panic will signal reader to fail with `BrokenPipe` error.
pub struct ArmedFuse<'a>(MutexGuard<'a, ()>);

/// Fuse that can be armed.
pub struct Fuse(Arc<Mutex<()>>);

impl Fuse {
    /// Arms the fuse.
    ///
    /// Returns `BrokenPipe` error if reader was dropped du to panic.
    pub fn arm(&self) -> Result<ArmedFuse, IoError> {
        self.0.lock().map(ArmedFuse).map_err(|_| IoError::new(ErrorKind::BrokenPipe, "reader end dropped due to panic"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::io::Write;
    use ringtail::io::pipe;

    #[test]
    fn test_unfused_panic() {
        let (mut reader, mut writer) = pipe();

        thread::spawn(move || {
            writer.write(&[1]).unwrap();
            panic!("boom");
        });

        let mut data = Vec::new();

        assert!(reader.read_to_end(&mut data).is_ok());
        assert_eq!(&data, &[1]);
    }

    #[test]
    fn test_fused_nopanic() {

        let (reader, mut writer) = pipe();

        let (mut reader, fuse) = fuse(reader);

        thread::spawn(move || {
            let _fuse = fuse.arm().unwrap();
            writer.write(&[1]).unwrap();
        });

        let mut data = Vec::new();

        assert!(reader.read_to_end(&mut data).is_ok());
        assert_eq!(&data, &[1]);
    }

    #[test]
    fn test_fused_panic() {

        let (reader, mut writer) = pipe();

        let (mut reader, fuse) = fuse(reader);

        thread::spawn(move || {
            let _fuse = fuse.arm().unwrap();
            writer.write(&[1]).unwrap();
            panic!("boom");
        });

        let mut data = Vec::new();

        assert!(reader.read_to_end(&mut data).is_err());
        assert_eq!(&data, &[1]);
    }
}
