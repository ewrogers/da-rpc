use std::{
    fmt,
    fs::{File, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const ROTATION_RECORD: &[u8] = b"event=log_rotated reason=size_limit\n";

#[derive(Clone)]
pub(crate) struct LogFile {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    file: File,
    bytes_written: u64,
    max_bytes: u64,
    rotate_before_write: bool,
}

impl LogFile {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_limit(path, MAX_LOG_BYTES)
    }

    fn open_with_limit(path: &Path, max_bytes: u64) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.seek(SeekFrom::End(0))?;
        let bytes_written = file.metadata()?.len();
        let mut inner = Inner {
            file,
            bytes_written,
            max_bytes,
            rotate_before_write: false,
        };

        if bytes_written >= max_bytes {
            inner.rotate()?;
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| io::Error::other("log file lock is poisoned"))
    }
}

impl Inner {
    fn rotate(&mut self) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(ROTATION_RECORD)?;
        self.bytes_written = ROTATION_RECORD.len() as u64;
        self.rotate_before_write = false;
        Ok(())
    }
}

impl Write for LogFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut inner = self.lock()?;
        if inner.rotate_before_write {
            inner.rotate()?;
        }

        let written = inner.file.write(buffer)?;
        inner.bytes_written = inner.bytes_written.saturating_add(written as u64);
        if inner.bytes_written >= inner.max_bytes && buffer[..written].ends_with(b"\n") {
            inner.rotate_before_write = true;
        }

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.lock()?.file.flush()
    }

    fn write_fmt(&mut self, arguments: fmt::Arguments<'_>) -> io::Result<()> {
        let mut record = String::new();
        fmt::write(&mut record, arguments)
            .map_err(|_| io::Error::other("failed to format log record"))?;
        self.write_all(record.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{LogFile, ROTATION_RECORD};
    use std::{
        fs,
        io::Write,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "darpc-log-file-test-{}-{}.log",
            process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn default_log_accepts_a_record() {
        let path = test_path();
        let mut log = LogFile::open(&path).expect("failed to open test log");

        writeln!(log, "event=test").expect("failed to write test record");

        let contents = fs::read(&path).expect("failed to read test log");
        assert_eq!(contents, b"event=test\n");
        fs::remove_file(path).expect("failed to remove test log");
    }

    #[test]
    fn rotates_before_the_record_after_the_limit() {
        let path = test_path();
        let mut log = LogFile::open_with_limit(&path, 12).expect("failed to open test log");

        writeln!(log, "first").expect("failed to write first record");
        writeln!(log, "second").expect("failed to write second record");
        writeln!(log, "third").expect("failed to write third record");

        let contents = fs::read(&path).expect("failed to read test log");
        assert_eq!(contents, [ROTATION_RECORD, b"third\n"].concat());
        fs::remove_file(path).expect("failed to remove test log");
    }

    #[test]
    fn clones_share_the_rotation_boundary() {
        let path = test_path();
        let mut first = LogFile::open_with_limit(&path, 5).expect("failed to open test log");
        let mut second = first.clone();

        writeln!(first, "first").expect("failed to write first record");
        writeln!(second, "second").expect("failed to write second record");

        let contents = fs::read(&path).expect("failed to read test log");
        assert_eq!(contents, [ROTATION_RECORD, b"second\n"].concat());
        fs::remove_file(path).expect("failed to remove test log");
    }
}
