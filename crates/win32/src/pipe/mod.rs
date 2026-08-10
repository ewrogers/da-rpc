use darpc_protocol::{
    FRAME_HEADER_LEN, Frame, MAX_FRAME_LEN, decode_frame, decode_header, encode_frame,
};
use std::{
    io,
    ptr::{null, null_mut},
    sync::Arc,
    time::Duration,
};
use windows_sys::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_PIPE_CONNECTED,
            ERROR_PIPE_NOT_CONNECTED, FALSE, GENERIC_READ, GENERIC_WRITE, HANDLE,
            INVALID_HANDLE_VALUE, LocalFree, TRUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Media::timeGetTime,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
            PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
            },
            Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject},
        },
    },
    core::BOOL,
};

const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(5);
const PIPE_SECURITY_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)";

#[must_use]
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\da-rpc-{pid}")
}

#[must_use]
pub fn sender_tick_ms() -> u32 {
    // SAFETY: timeGetTime takes no arguments and has no caller-owned state.
    unsafe { timeGetTime() }
}

#[derive(Clone)]
pub struct StopEvent {
    handle: Arc<OwnedHandle>,
}

impl StopEvent {
    pub fn new() -> io::Result<Self> {
        // SAFETY: Null security and name pointers request an unnamed,
        // non-inheritable event. BOOL arguments are valid constants.
        let handle = unsafe { CreateEventW(null(), TRUE, FALSE, null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: Arc::new(OwnedHandle(handle)),
        })
    }

    pub fn signal(&self) -> io::Result<()> {
        // SAFETY: The Arc keeps this valid event handle alive for the call.
        if unsafe { SetEvent(self.handle.raw()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[must_use]
    pub fn is_signaled(&self) -> bool {
        // SAFETY: The Arc keeps this valid event handle alive for the call.
        unsafe { WaitForSingleObject(self.handle.raw(), 0) == WAIT_OBJECT_0 }
    }

    fn raw(&self) -> HANDLE {
        self.handle.raw()
    }
}

pub struct PipeServer {
    stream: PipeStream,
}

impl PipeServer {
    pub fn bind(pid: u32, stop: StopEvent) -> io::Result<Self> {
        let name = wide(&pipe_name(pid));
        let mut security = PipeSecurity::new()?;
        let attributes = security.attributes();
        let buffer_size = u32::try_from(MAX_FRAME_LEN)
            .map_err(|_| io::Error::other("maximum frame length does not fit u32"))?;

        // SAFETY: The name and security descriptor remain alive for the call.
        // All flags and sizes satisfy the CreateNamedPipeW contract.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                buffer_size,
                buffer_size,
                0,
                &raw const attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            stream: PipeStream::new(OwnedHandle(handle), Some(stop), u32::MAX),
        })
    }

    pub fn accept(&self) -> io::Result<()> {
        let event = OwnedHandle::event()?;
        let mut overlapped = OVERLAPPED {
            hEvent: event.raw(),
            ..OVERLAPPED::default()
        };

        // SAFETY: The pipe was created for overlapped I/O. The OVERLAPPED and
        // its event remain alive until the operation completes.
        if unsafe { ConnectNamedPipe(self.stream.handle.raw(), &raw mut overlapped) } != 0 {
            return Ok(());
        }

        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|value| value as u32) {
            Some(ERROR_PIPE_CONNECTED) => Ok(()),
            Some(ERROR_IO_PENDING) => self.stream.wait_for_pending(&mut overlapped).map(|_| ()),
            _ => Err(error),
        }
    }

    pub fn receive_frame(&self) -> io::Result<Frame> {
        self.stream.receive_frame()
    }

    pub fn send_frame(&self, frame: &Frame) -> io::Result<()> {
        self.stream.send_frame(frame)
    }

    pub fn disconnect(&self) -> io::Result<()> {
        // SAFETY: This is the server handle returned by CreateNamedPipeW.
        if unsafe { DisconnectNamedPipe(self.stream.handle.raw()) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error().map(|value| value as u32) == Some(ERROR_PIPE_NOT_CONNECTED) {
            return Ok(());
        }
        Err(error)
    }
}

pub struct PipeClient {
    stream: PipeStream,
}

impl PipeClient {
    pub fn connect(pid: u32) -> io::Result<Self> {
        let name = wide(&pipe_name(pid));

        // SAFETY: The generated pipe name remains alive for the call. The
        // remaining pointer and handle arguments are documented null values.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            stream: PipeStream::new(
                OwnedHandle(handle),
                None,
                duration_millis(CLIENT_IO_TIMEOUT),
            ),
        })
    }

    pub fn receive_frame(&self) -> io::Result<Frame> {
        self.stream.receive_frame()
    }

    pub fn send_frame(&self, frame: &Frame) -> io::Result<()> {
        self.stream.send_frame(frame)
    }
}

struct PipeStream {
    handle: OwnedHandle,
    stop: Option<StopEvent>,
    timeout_ms: u32,
}

impl PipeStream {
    const fn new(handle: OwnedHandle, stop: Option<StopEvent>, timeout_ms: u32) -> Self {
        Self {
            handle,
            stop,
            timeout_ms,
        }
    }

    fn receive_frame(&self) -> io::Result<Frame> {
        let mut header_bytes = [0_u8; FRAME_HEADER_LEN];
        self.read_exact(&mut header_bytes)?;
        let header = decode_header(&header_bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let frame_len = header
            .frame_len()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        let mut frame_bytes = Vec::with_capacity(frame_len);
        frame_bytes.extend_from_slice(&header_bytes);
        frame_bytes.resize(frame_len, 0);
        self.read_exact(&mut frame_bytes[FRAME_HEADER_LEN..])?;

        decode_frame(&frame_bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn send_frame(&self, frame: &Frame) -> io::Result<()> {
        let bytes = encode_frame(frame)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.write_all(&bytes)
    }

    fn read_exact(&self, mut buffer: &mut [u8]) -> io::Result<()> {
        while !buffer.is_empty() {
            let transferred = self.read(buffer)?;
            if transferred == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "named pipe closed during frame read",
                ));
            }
            buffer = &mut buffer[transferred..];
        }
        Ok(())
    }

    fn write_all(&self, mut buffer: &[u8]) -> io::Result<()> {
        while !buffer.is_empty() {
            let transferred = self.write(buffer)?;
            if transferred == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "named pipe wrote zero frame bytes",
                ));
            }
            buffer = &buffer[transferred..];
        }
        Ok(())
    }

    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = u32::try_from(buffer.len())
            .map_err(|_| io::Error::other("read length does not fit u32"))?;
        self.transfer(|overlapped, transferred| {
            // SAFETY: The mutable buffer is valid for `length` bytes and stays
            // alive until this overlapped operation completes.
            unsafe {
                ReadFile(
                    self.handle.raw(),
                    buffer.as_mut_ptr(),
                    length,
                    transferred,
                    overlapped,
                )
            }
        })
    }

    fn write(&self, buffer: &[u8]) -> io::Result<usize> {
        let length = u32::try_from(buffer.len())
            .map_err(|_| io::Error::other("write length does not fit u32"))?;
        self.transfer(|overlapped, transferred| {
            // SAFETY: The immutable buffer is valid for `length` bytes and
            // stays alive until this overlapped operation completes.
            unsafe {
                WriteFile(
                    self.handle.raw(),
                    buffer.as_ptr(),
                    length,
                    transferred,
                    overlapped,
                )
            }
        })
    }

    fn transfer(
        &self,
        operation: impl FnOnce(*mut OVERLAPPED, *mut u32) -> BOOL,
    ) -> io::Result<usize> {
        let event = OwnedHandle::event()?;
        let mut overlapped = OVERLAPPED {
            hEvent: event.raw(),
            ..OVERLAPPED::default()
        };
        let mut transferred = 0_u32;

        if operation(&raw mut overlapped, &raw mut transferred) != 0 {
            return Ok(transferred as usize);
        }

        let error = io::Error::last_os_error();
        if error.raw_os_error().map(|value| value as u32) != Some(ERROR_IO_PENDING) {
            return Err(error);
        }

        self.wait_for_pending(&mut overlapped)
            .map(|transferred| transferred as usize)
    }

    fn wait_for_pending(&self, overlapped: &mut OVERLAPPED) -> io::Result<u32> {
        let event = overlapped.hEvent;
        let wait = if let Some(stop) = &self.stop {
            let handles = [stop.raw(), event];
            // SAFETY: Both handles remain alive for the entire wait and the
            // array contains exactly the declared number of handles.
            unsafe {
                WaitForMultipleObjects(
                    u32::try_from(handles.len()).expect("two handles fit u32"),
                    handles.as_ptr(),
                    FALSE,
                    self.timeout_ms,
                )
            }
        } else {
            // SAFETY: The operation event remains alive for the entire wait.
            unsafe { WaitForSingleObject(event, self.timeout_ms) }
        };

        let completed = if self.stop.is_some() {
            WAIT_OBJECT_0 + 1
        } else {
            WAIT_OBJECT_0
        };
        if wait == completed {
            return self.overlapped_result(overlapped, FALSE);
        }
        if wait == WAIT_OBJECT_0 && self.stop.is_some() {
            self.cancel_and_wait(overlapped);
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "named pipe operation stopped",
            ));
        }
        if wait == WAIT_TIMEOUT {
            self.cancel_and_wait(overlapped);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "named pipe operation timed out",
            ));
        }
        if wait == WAIT_FAILED {
            let error = io::Error::last_os_error();
            self.cancel_and_wait(overlapped);
            return Err(error);
        }

        self.cancel_and_wait(overlapped);
        Err(io::Error::other(format!(
            "unexpected named pipe wait result {wait}"
        )))
    }

    fn cancel_and_wait(&self, overlapped: &mut OVERLAPPED) {
        // SAFETY: This is the same live handle and OVERLAPPED used to start the
        // pending operation. The structure remains alive through completion.
        let cancelled = unsafe { CancelIoEx(self.handle.raw(), &raw const *overlapped) };
        if cancelled == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error().map(|value| value as u32) != Some(ERROR_NOT_FOUND) {
                // Completion is still collected below so OVERLAPPED storage is
                // never released while the kernel may reference it.
            }
        }
        let _ = self.overlapped_result(overlapped, TRUE);
    }

    fn overlapped_result(&self, overlapped: &mut OVERLAPPED, wait: BOOL) -> io::Result<u32> {
        let mut transferred = 0_u32;
        // SAFETY: The handle and OVERLAPPED belong to the same operation and
        // remain alive for the call. The output pointer is writable.
        if unsafe {
            GetOverlappedResult(
                self.handle.raw(),
                &raw const *overlapped,
                &raw mut transferred,
                wait,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(transferred)
    }
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn event() -> io::Result<Self> {
        // SAFETY: Null security and name pointers request an unnamed,
        // non-inheritable event. BOOL arguments are valid constants.
        let handle = unsafe { CreateEventW(null(), TRUE, FALSE, null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    const fn raw(&self) -> HANDLE {
        self.0
    }
}

// SAFETY: Windows kernel handles can be used from other threads. Ownership is
// retained by this wrapper and CloseHandle runs exactly once on drop.
unsafe impl Send for OwnedHandle {}
// SAFETY: The wrapped event and pipe operations are synchronized by Windows;
// the wrapper exposes no mutable Rust references to handle-owned state.
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns a valid, non-pseudo handle.
        unsafe { CloseHandle(self.0) };
    }
}

struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl PipeSecurity {
    fn new() -> io::Result<Self> {
        let sddl = wide(PIPE_SECURITY_SDDL);
        let mut descriptor = null_mut();
        // SAFETY: The SDDL is null-terminated and all output pointers are valid
        // for the call. LocalFree releases a successful allocation in Drop.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { descriptor })
    }

    fn attributes(&mut self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .expect("SECURITY_ATTRIBUTES size fits u32"),
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: FALSE,
        }
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: ConvertStringSecurityDescriptor allocated this descriptor
        // with LocalAlloc and ownership has not been transferred.
        unsafe { LocalFree(self.descriptor) };
    }
}

fn duration_millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX - 1)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::{duration_millis, pipe_name};
    use std::time::Duration;

    #[test]
    fn derives_the_pid_pipe_name() {
        assert_eq!(pipe_name(42), r"\\.\pipe\da-rpc-42");
    }

    #[test]
    fn bounds_duration_conversion() {
        assert_eq!(duration_millis(Duration::from_secs(5)), 5_000);
        assert_eq!(duration_millis(Duration::MAX), u32::MAX - 1);
    }
}
