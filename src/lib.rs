use std::io::{Read, Error as IoError, ErrorKind};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

/// Fuses reader so that if writer thread dies while holding armed fuse the reader will get `BrokenPipe` error.
pub fn fuse<R: Read>(reader: R) -> (FusedReader<R>, Fuse) {
    let reader_fuse = Arc::new(Mutex::new(Ok(())));
    let writer_fuse = reader_fuse.clone();
( FusedReader {
            reader,
            fuse: reader_fuse,
        },
        Fuse(writer_fuse),
    )
}

/// Reader that will fail with I/O error if fuse was blown.
#[derive(Debug)]
pub struct FusedReader<R: Read> {
    reader: R,
    fuse: Arc<Mutex<Result<(), IoError>>>,
}

/// Status of the fuse.
#[derive(Debug)]
pub enum FuseStatus {
    /// Fuse was not armed or guard got dropped.
    Unarmed,
    /// Fuse armed.
    Armed,
    /// Fuse blown with error.
    Blown(IoError),
    /// Fuse blown due by unwind.
    Poisoned,
}

impl<R: Read> FusedReader<R> {
    /// Checks status of the fuse.
    ///
    /// Note that the variant `FuseStatus::Blown` is provided only once and following calls will
    /// return `FuseStatus::Unarmed` instead.
    pub fn check_fuse(&mut self) -> FuseStatus {
        match self.fuse.try_lock() {
            Err(TryLockError::Poisoned(_)) => FuseStatus::Poisoned,
            Ok(mut guard) => {
                if guard.is_err() {
                    let mut res = Ok(());
                    std::mem::swap(&mut *guard, &mut res);
                    FuseStatus::Blown(res.unwrap_err())
                } else {
                    FuseStatus::Unarmed
                }
            }
            Err(TryLockError::WouldBlock) => FuseStatus::Armed,
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
        self.reader.read(buf).and_then(|bytes| if bytes == 0 {
            match self.check_fuse() {
                FuseStatus::Blown(err) => Err(err),
                FuseStatus::Poisoned => Err(IoError::new(ErrorKind::BrokenPipe, "writer end dropped due to panic")),
                FuseStatus::Unarmed |
                FuseStatus::Armed => Ok(bytes),
            }
        } else {
            Ok(bytes)
        })
    }
}

/// Fuse that can be armed.
#[derive(Debug)]
pub struct Fuse(Arc<Mutex<Result<(), IoError>>>);

impl Fuse {
    /// Arms the fuse.
    ///
    /// Returns `BrokenPipe` error if reader was dropped due to panic.
    pub fn arm(&self) -> Result<FuseGuard, IoError> {
        self.0.lock().map(FuseGuard).map_err(|_| IoError::new(ErrorKind::BrokenPipe, "reader end dropped due to panic"))
    }
}

/// Armed fuse that if dropped due to panic will signal reader to fail with `BrokenPipe` error.
#[derive(Debug)]
pub struct FuseGuard<'a>(MutexGuard<'a, Result<(), IoError>>);

impl<'a> FuseGuard<'a> {
    /// Blows the fuse with given error.
    ///
    /// The reader end will fail with this error after reaching EOF.
    pub fn blow(mut self, err: IoError) {
        *self.0 = Err(err);
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

    #[test]
    fn test_fused_blow() {

        let (reader, mut writer) = pipe();

        let (mut reader, fuse) = fuse(reader);

        thread::spawn(move || {
            let fuse = fuse.arm().unwrap();
            writer.write(&[1]).unwrap();
            fuse.blow(IoError::new(ErrorKind::BrokenPipe, "boom!"))
        });

        let mut data = Vec::new();

        assert!(reader.read_to_end(&mut data).is_err());
        assert_eq!(&data, &[1]);
    }
}
