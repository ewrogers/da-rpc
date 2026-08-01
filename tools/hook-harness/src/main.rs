//! Owned x86 process used to qualify daRPC's in-process detour mechanism.

use std::process::ExitCode;

#[cfg(not(all(windows, target_arch = "x86")))]
fn main() -> ExitCode {
    eprintln!("hook-harness requires an x86 Windows target");
    ExitCode::FAILURE
}

#[cfg(all(windows, target_arch = "x86"))]
fn main() -> ExitCode {
    match harness::run() {
        Ok(summary) => {
            println!(
                "hook harness passed: relocated_bytes={} observations={} concurrent_calls={}",
                summary.relocated_bytes, summary.observations, summary.concurrent_calls
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("hook-harness: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(all(windows, target_arch = "x86"))]
mod harness {
    use darpc_hook::{
        CodeRange, DetourActivity, DetourError, DetourSpec, InstalledDetour, PreparedDetour,
    };
    use std::{
        error::Error,
        io,
        panic::{self, AssertUnwindSafe},
        ptr::NonNull,
        sync::{
            Arc, Barrier,
            atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    const DETOUR_RANGE_LEN: usize = 64;
    const WORKER_COUNT: usize = 4;
    const RETRY_TIMEOUT: Duration = Duration::from_secs(5);

    #[unsafe(no_mangle)]
    static HOOK_ACTIVITY: DetourActivity = DetourActivity::new();
    static TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
    static OBSERVATIONS: AtomicU32 = AtomicU32::new(0);
    static FORCE_PANIC: AtomicBool = AtomicBool::new(false);

    type TargetFn = unsafe extern "C" fn(i32, i32) -> i32;

    #[unsafe(naked)]
    unsafe extern "C" fn deterministic_target(_left: i32, _right: i32) -> i32 {
        core::arch::naked_asm!(
            "call {multiply}",
            "add eax, dword ptr [esp + 8]",
            "ret",
            multiply = sym multiply_left_by_three,
        );
    }

    #[unsafe(naked)]
    unsafe extern "C" fn multiply_left_by_three(_left: i32, _right: i32) -> i32 {
        core::arch::naked_asm!("mov eax, dword ptr [esp + 8]", "imul eax, eax, 3", "ret",);
    }

    #[unsafe(naked)]
    unsafe extern "C" fn deterministic_detour(_left: i32, _right: i32) -> i32 {
        core::arch::naked_asm!(
            "lock inc dword ptr [{activity}]",
            "push dword ptr [esp + 8]",
            "push dword ptr [esp + 8]",
            "call {body}",
            "add esp, 8",
            "lock dec dword ptr [{activity}]",
            "ret",
            activity = sym HOOK_ACTIVITY,
            body = sym detour_body,
        );
    }

    extern "C" fn detour_body(left: i32, right: i32) -> i32 {
        panic::catch_unwind(AssertUnwindSafe(|| {
            if FORCE_PANIC.swap(false, Ordering::AcqRel) {
                panic!("injected detour panic");
            }

            OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
            let trampoline = TRAMPOLINE.load(Ordering::Acquire);
            if trampoline == 0 {
                return expected(left, right);
            }

            // SAFETY: the pointer was published from a live PreparedDetour and
            // has the exact ABI of the original deterministic target.
            let original: TargetFn = unsafe { std::mem::transmute(trampoline) };
            // SAFETY: the trampoline preserves the target ABI and calls back
            // into the original function after its relocated prologue.
            unsafe { original(left, right) }
        }))
        .unwrap_or_else(|_| expected(left, right))
    }

    pub(super) struct Summary {
        pub(super) relocated_bytes: usize,
        pub(super) observations: u32,
        pub(super) concurrent_calls: u64,
    }

    pub(super) fn run() -> Result<Summary, Box<dyn Error>> {
        verify_results("before installation")?;

        let mut prepared = prepare()?;
        let relocated_bytes = prepared.relocated_len();
        TRAMPOLINE.store(prepared.trampoline_address()?, Ordering::Release);

        // SAFETY: this repeats the same valid fixture specification and must
        // stop at the reservation before reading or changing target code.
        let duplicate = unsafe { PreparedDetour::prepare(spec()?) }
            .err()
            .ok_or_else(|| io::Error::other("duplicate preparation unexpectedly succeeded"))?;
        ensure(
            matches!(duplicate, DetourError::AlreadyReserved { .. }),
            "duplicate preparation returned the wrong error",
        )?;

        let mut installed = prepared.install()?;
        ensure(installed.is_installed(), "detour did not report installed")?;
        verify_results("during installation")?;

        let observations_before = OBSERVATIONS.load(Ordering::Relaxed);
        let direct_result = call_target(17, -9);
        ensure(
            direct_result == expected(17, -9),
            "trampoline changed output",
        )?;
        ensure(
            OBSERVATIONS.load(Ordering::Relaxed) == observations_before.wrapping_add(1),
            "original call recursively re-entered the detour",
        )?;

        verify_panic_boundary()?;
        ensure(installed.uninstall()?, "first uninstall reported no change")?;
        TRAMPOLINE.store(0, Ordering::Release);
        ensure(
            !installed.uninstall()?,
            "repeated uninstall reported a change",
        )?;
        verify_results("after first removal")?;

        let stop = Arc::new(AtomicBool::new(false));
        let failures = Arc::new(AtomicU32::new(0));
        let calls = Arc::new(AtomicU64::new(0));
        let start = Arc::new(Barrier::new(WORKER_COUNT + 1));
        let workers = start_workers(
            Arc::clone(&stop),
            Arc::clone(&failures),
            Arc::clone(&calls),
            Arc::clone(&start),
        );
        start.wait();

        let mut prepared = prepare()?;
        TRAMPOLINE.store(prepared.trampoline_address()?, Ordering::Release);
        let mut installed = install_with_retry(&mut prepared)?;
        wait_for_observations(observations_before.wrapping_add(500))?;
        uninstall_with_retry(&mut installed)?;
        TRAMPOLINE.store(0, Ordering::Release);

        let observations_after_removal = OBSERVATIONS.load(Ordering::Acquire);
        thread::sleep(Duration::from_millis(50));
        ensure(
            OBSERVATIONS.load(Ordering::Acquire) == observations_after_removal,
            "detour was called after transactional removal",
        )?;

        stop.store(true, Ordering::Release);
        for worker in workers {
            worker
                .join()
                .map_err(|_| io::Error::other("concurrent worker panicked"))?;
        }
        ensure(
            failures.load(Ordering::Acquire) == 0,
            "concurrent target call returned an incorrect result",
        )?;
        ensure(
            HOOK_ACTIVITY.active_calls() == 0,
            "detour activity remained after shutdown",
        )?;
        verify_results("after concurrent shutdown")?;

        Ok(Summary {
            relocated_bytes,
            observations: observations_after_removal,
            concurrent_calls: calls.load(Ordering::Acquire),
        })
    }

    fn prepare() -> Result<PreparedDetour, DetourError> {
        // SAFETY: the fixture functions are owned, executable x86 routines.
        // The detour's assembly updates HOOK_ACTIVITY before leaving its
        // declared entry range and immediately before returning.
        unsafe { PreparedDetour::prepare(spec()?) }
    }

    fn spec() -> Result<DetourSpec, DetourError> {
        let target =
            NonNull::new(deterministic_target as *mut u8).ok_or(DetourError::InvalidCodeRange)?;
        let detour =
            NonNull::new(deterministic_detour as *mut u8).ok_or(DetourError::InvalidCodeRange)?;
        DetourSpec::new(
            target,
            detour,
            CodeRange::new(detour.as_ptr() as usize, DETOUR_RANGE_LEN)?,
            &HOOK_ACTIVITY,
        )
    }

    fn install_with_retry(prepared: &mut PreparedDetour) -> Result<InstalledDetour, DetourError> {
        let deadline = Instant::now() + RETRY_TIMEOUT;
        loop {
            match prepared.install() {
                Ok(installed) => return Ok(installed),
                Err(error) if error.is_transient() && Instant::now() < deadline => {
                    thread::yield_now();
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn uninstall_with_retry(installed: &mut InstalledDetour) -> Result<(), DetourError> {
        let deadline = Instant::now() + RETRY_TIMEOUT;
        loop {
            match installed.uninstall() {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(DetourError::InvalidState),
                Err(error) if error.is_transient() && Instant::now() < deadline => {
                    thread::yield_now();
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn start_workers(
        stop: Arc<AtomicBool>,
        failures: Arc<AtomicU32>,
        calls: Arc<AtomicU64>,
        start: Arc<Barrier>,
    ) -> Vec<thread::JoinHandle<()>> {
        (0..WORKER_COUNT)
            .map(|worker| {
                let stop = Arc::clone(&stop);
                let failures = Arc::clone(&failures);
                let calls = Arc::clone(&calls);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    start.wait();
                    let mut iteration = worker as i32;
                    while !stop.load(Ordering::Acquire) {
                        let left = iteration.wrapping_mul(17).wrapping_add(3);
                        let right = iteration.wrapping_mul(-5).wrapping_sub(11);
                        if call_target(left, right) != expected(left, right) {
                            failures.fetch_add(1, Ordering::Relaxed);
                            stop.store(true, Ordering::Release);
                            break;
                        }
                        calls.fetch_add(1, Ordering::Relaxed);
                        iteration = iteration.wrapping_add(1);
                        thread::yield_now();
                    }
                })
            })
            .collect()
    }

    fn wait_for_observations(minimum: u32) -> Result<(), io::Error> {
        let deadline = Instant::now() + RETRY_TIMEOUT;
        while OBSERVATIONS.load(Ordering::Acquire) < minimum {
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "timed out waiting for concurrent detour observations",
                ));
            }
            thread::yield_now();
        }
        Ok(())
    }

    fn verify_results(context: &str) -> Result<(), io::Error> {
        for (left, right) in [(0, 0), (1, 2), (-7, 13), (i32::MAX, i32::MIN)] {
            if call_target(left, right) != expected(left, right) {
                return Err(io::Error::other(format!(
                    "target result changed {context} for ({left}, {right})"
                )));
            }
        }
        Ok(())
    }

    fn verify_panic_boundary() -> Result<(), io::Error> {
        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        FORCE_PANIC.store(true, Ordering::Release);
        let actual = call_target(9, 4);
        panic::set_hook(previous_hook);
        ensure(
            actual == expected(9, 4),
            "caught detour panic changed target output",
        )
    }

    fn call_target(left: i32, right: i32) -> i32 {
        // SAFETY: deterministic_target has the declared cdecl-compatible ABI
        // and remains executable for the complete harness lifetime.
        unsafe { deterministic_target(left, right) }
    }

    fn expected(left: i32, right: i32) -> i32 {
        left.wrapping_mul(3).wrapping_add(right)
    }

    fn ensure(condition: bool, message: &str) -> Result<(), io::Error> {
        condition
            .then_some(())
            .ok_or_else(|| io::Error::other(message))
    }
}
