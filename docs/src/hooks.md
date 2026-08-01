# Hook safety

daRPC uses a small in-process x86 detour implementation in the `darpc-hook`
crate. The implementation is qualified against owned code before it is used
with the game client. The current runtime uses it for the client tick and map
size handler hooks.

## Organization

`darpc-hook` contains the platform-specific mechanism. It knows how to decode
an x86 prologue, prepare a trampoline, enlist threads, replace code, and restore
the original bytes. It contains no Dark Ages addresses, calling conventions,
or state logic.

`hook-harness.exe` owns deterministic x86 target and detour routines. Its
target computes `left * 3 + right` with wrapping 32-bit arithmetic. The harness
checks the same inputs before installation, through the detour, and after
removal while retaining only a bounded observation counter.

Client hooks belong in `darpc.dll`. They supply validated addresses, the exact
native ABI, a detour code range, and an activity counter to the shared
mechanism.

## Client tick hook

The DLL installs the tick hook only after validating the exact supported
executable fingerprint. It derives the target from the executable module base
and the version-specific relative virtual address, then verifies the expected
entry bytes before preparing or changing code. A mismatch fails DLL
initialization and rolls back the IPC worker.

The x86 detour preserves the target's `thiscall` receiver, increments a
wrapping atomic `u32` counter, calls the original function through its relocated
trampoline, and returns normally. The hook path takes no lock, allocates no
memory, performs no I/O, and calls no unrelated client function.

The DLL worker reads the atomic health state for `TickHealthRequest` messages
and writes diagnostic samples to
`%USERPROFILE%\darpc\logs\pid-<pid>.log`. `darpc.exe ipc tick-health --pid
<pid>` samples twice and reports installation, relocated-byte, counter, and
advancement fields. Logging and named-pipe work never execute in the hook.

The same tick detour also observes pending snapshot requests. It performs the
bounded main-thread memory copy described in [Client state](state.md), then
publishes pointer-free fixed-capacity data for the pipe worker. Conversion and
serialization remain outside the hook.

## Map-size hook

The map-size handler hook uses the same fingerprint, entry-byte, relocation,
thread enlistment, activity tracking, and removal guarantees. Its synchronous
callback copies only the map identifier and bounded inline name into atomic
DLL-owned storage before calling the original handler. It retains no packet or
client pointer after the callback. A panic is caught at the callback boundary
and never crosses the native client application binary interface.

The current snapshot walker normally reads the map name from the validated map
pane. The event-owned copy supplements that baseline only when its world token
and map identifier match the captured location.

## Preparation and relocation

Preparation does not modify the target. It performs these steps first:

1. Reserve the target so another daRPC detour cannot prepare over it.
2. Decode complete instructions until at least the five bytes required by an
   x86 near jump are covered. Reject a return, interrupt, exception, or
   unconditional transfer that ends a shorter function before that boundary.
3. Allocate writable trampoline memory.
4. Re-encode the decoded instructions at the trampoline address with
   `iced-x86`, allowing relative branches and calls to receive correct
   displacements.
5. Append a near jump from the trampoline back to the first untouched target
   instruction.
6. Change the complete trampoline from writable to executable and read-only,
   then flush the process instruction cache.

The owned fixture deliberately begins with a five-byte relative `call`. The
harness therefore exercises relocation rather than merely copying
position-independent instructions. The decoded replacement length is reported
by the harness and asserted to be five bytes for this reviewed fixture.

Callers must guarantee that the target range is readable executable x86 code,
that the target and detour have the same application binary interface (ABI),
and that no external branch enters the interior of the replaced prologue.
Installation and removal must be initiated by a dedicated lifecycle or
management thread, never from the target, detour, or trampoline call path. The
commit code cannot suspend or inspect its own instruction pointer.

## Transactional commit

Installation and removal use the same short commit boundary:

1. Enumerate and suspend every other thread owned by the current process.
2. Repeat enumeration until no newly created thread remains unenlisted.
3. Read each suspended thread's instruction pointer.
4. Reject installation if a thread is inside the target range.
5. Reject removal if a thread is inside the target, detour, or trampoline, or
   if the detour activity counter is nonzero.
6. Confirm the target still contains the expected bytes.
7. Open the target page for execution and writing, replace the complete
   instruction range, flush the instruction cache, and restore protection.
8. Resume every enlisted thread, including on an error path.

All allocation, instruction decoding, relocation, and trampoline writes happen
before thread enlistment. The commit path performs no Rust heap allocation.
A process with more than 256 enlistable threads is rejected instead of growing
storage while other threads are suspended.

Every resume is checked and retried. A thread that exited while enlisted is no
longer considered suspended; a live thread that still cannot be resumed is a
lifecycle failure. Successful installation retains detour ownership even when
resumption reports a warning. DLL initialization then returns `UNLOAD_UNSAFE`
while keeping the lifecycle state, so the loader cannot accidentally discard a
live trampoline or target reservation.

If cache flushing or protection restoration fails after a target write, the
transaction restores the original bytes, flushes them, and restores the prior
protection before returning an error. A native unit test injects a failure
immediately after the write and verifies byte-exact rollback.

If rollback itself cannot be confirmed, DLL initialization returns the
`UNLOAD_UNSAFE` lifecycle status. A late-attach loader leaves the DLL mapped
instead of calling `FreeLibrary` against potentially referenced hook code.

## Detour lifetime

The detour must increment its `DetourActivity` counter before execution can
leave the declared detour entry range. It decrements the counter only
immediately before returning. Removal suspends other threads and requires both
an empty activity count and instruction pointers outside every hook-owned code
range. Once the original target bytes are restored, no new detour call can
begin and the trampoline may be released after the suspended threads resume.

An installed detour must be explicitly removed before its owning DLL unloads.
If removal reports a transient active-call or instruction-pointer conflict,
shutdown must retry while the DLL remains loaded. Dropping an installed detour
does not free its trampoline; it intentionally leaks the allocation and target
reservation so an accidental Rust drop cannot create an immediate dangling
jump. The lifecycle owner must still refuse DLL unload until removal succeeds.

The owned detour catches Rust panics inside its native boundary and returns the
original deterministic result. Production detours must apply the same rule:
no panic or unwind may cross a native client ABI.

## Qualification coverage

Windows continuous integration runs the following on an x86 target:

- decoder, code-range, reservation, and injected rollback unit tests;
- deterministic target-instruction-pointer, active-call, and resume-failure
  ownership tests;
- Clippy for the hook crate and harness;
- debug and release harness executions;
- repeated preparation and removal checks;
- original-call recursion detection;
- a caught-panic boundary check;
- concurrent installation and removal with four target callers;
- byte-exact results before, during, and after the detour; and
- proof that the observation counter stops changing after removal while target
  calls continue.

These checks qualify the mechanism. Each client hook additionally requires an
exact client fingerprint, target address, entry-byte contract, native ABI,
repeatable clean removal, and continued normal client behavior. The tick hook
also requires advancing live-client health samples.
