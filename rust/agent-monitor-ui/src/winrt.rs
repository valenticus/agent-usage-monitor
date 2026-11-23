//! Explicit WinRT apartment initialization for threads that call WinRT APIs
//! (MSIX build only: `ApplicationData`, `StartupTask`).
//!
//! `RoInitialize(RO_INIT_MULTITHREADED)` on the calling thread:
//!
//! * `S_OK` / `S_FALSE` (newly initialized, or already in the MTA): this guard
//!   owns one reference and calls `RoUninitialize` exactly once on drop, as
//!   every successful `RoInitialize` must be balanced.
//! * `RPC_E_CHANGED_MODE`: the thread already belongs to a single-threaded
//!   apartment initialized by other code. The guard does not own it and never
//!   uninitializes it. Callers that must block on WinRT async operations treat
//!   this as an error ([`Apartment::require_multithreaded`]); blocking in an STA
//!   could deadlock.
//! * Any other failure is returned as an error.
//!
//! AUM only uses the guard on threads it creates for that purpose, so it never
//! touches the apartment of the Slint UI thread.

/// How the calling thread's apartment was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitOutcome {
    /// `RoInitialize` succeeded; this guard must call `RoUninitialize`.
    Owned,
    /// Another component already initialized a different (STA) apartment.
    ForeignSingleThreaded,
}

/// Maps an `RoInitialize` HRESULT to an outcome.
pub fn classify(hresult: i32) -> Result<InitOutcome, String> {
    const S_OK: i32 = 0;
    const S_FALSE: i32 = 1;
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;
    match hresult {
        S_OK | S_FALSE => Ok(InitOutcome::Owned),
        RPC_E_CHANGED_MODE => Ok(InitOutcome::ForeignSingleThreaded),
        other => Err(format!("RoInitialize failed: 0x{:08X}", other as u32)),
    }
}

/// RAII guard for the calling thread's WinRT apartment. Not `Send`: it must be
/// dropped on the thread that created it.
pub struct Apartment {
    outcome: InitOutcome,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Apartment {
    /// Joins (or creates) the multithreaded apartment on this thread.
    #[cfg(windows)]
    pub fn multithreaded() -> Result<Self, String> {
        use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
        // SAFETY: plain apartment initialization of the calling thread; the
        // matching RoUninitialize happens in Drop on the same thread.
        let hresult = match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => 0,
            Err(error) => error.code().0,
        };
        Ok(Self {
            outcome: classify(hresult)?,
            _not_send: std::marker::PhantomData,
        })
    }

    /// Like [`Apartment::multithreaded`], but fails when the thread is
    /// already in a single-threaded apartment (blocking waits are not allowed
    /// there).
    #[cfg(windows)]
    pub fn require_multithreaded() -> Result<Self, String> {
        let apartment = Self::multithreaded()?;
        match apartment.outcome {
            InitOutcome::Owned => Ok(apartment),
            InitOutcome::ForeignSingleThreaded => {
                Err("thread is already in a single-threaded apartment".into())
            }
        }
    }

    #[cfg(test)]
    pub fn outcome(&self) -> InitOutcome {
        self.outcome
    }
}

#[cfg(windows)]
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.outcome == InitOutcome::Owned {
            // SAFETY: balances the successful RoInitialize of this guard, on
            // the same thread (the guard is not Send).
            unsafe { windows::Win32::System::WinRT::RoUninitialize() };
        }
    }
}

/// Runs `work` on a new thread inside its own multithreaded apartment and
/// waits for the result. Used for short synchronous WinRT reads before the UI
/// event loop exists (package data folder), so no existing apartment is ever
/// changed.
#[cfg(windows)]
pub fn run_in_multithreaded_apartment<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    std::thread::Builder::new()
        .name("aum-winrt".into())
        .spawn(move || {
            let _apartment = Apartment::require_multithreaded()?;
            work()
        })
        .map_err(|error| format!("WinRT worker: {error}"))?
        .join()
        .map_err(|_| "WinRT worker panicked".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_results_are_classified() {
        assert_eq!(classify(0), Ok(InitOutcome::Owned));
        assert_eq!(classify(1), Ok(InitOutcome::Owned));
        assert_eq!(
            classify(0x8001_0106_u32 as i32),
            Ok(InitOutcome::ForeignSingleThreaded)
        );
        // E_OUTOFMEMORY, E_INVALIDARG: reported, never treated as initialized.
        assert!(classify(0x8007_000E_u32 as i32).is_err());
        assert!(classify(0x8007_0057_u32 as i32).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn nested_guards_balance_on_a_fresh_thread() {
        std::thread::spawn(|| {
            let outer = Apartment::require_multithreaded().unwrap();
            let inner = Apartment::multithreaded().unwrap();
            assert_eq!(outer.outcome(), InitOutcome::Owned);
            // S_FALSE: already in the MTA, still owned once and balanced.
            assert_eq!(inner.outcome(), InitOutcome::Owned);
            drop(inner);
            drop(outer);
            // Fully uninitialized again: a new guard initializes afresh.
            assert_eq!(
                Apartment::require_multithreaded().unwrap().outcome(),
                InitOutcome::Owned
            );
        })
        .join()
        .unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_foreign_single_threaded_apartment_is_detected_and_left_alone() {
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
        };
        std::thread::spawn(|| {
            // SAFETY: test thread owned by this test; balanced below.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
            let foreign = Apartment::multithreaded().unwrap();
            assert_eq!(foreign.outcome(), InitOutcome::ForeignSingleThreaded);
            drop(foreign); // must not uninitialize the STA
            assert!(Apartment::require_multithreaded().is_err());
            // SAFETY: balances the CoInitializeEx above (still exactly one).
            unsafe { CoUninitialize() };
        })
        .join()
        .unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn worker_runs_inside_its_own_apartment() {
        let value = run_in_multithreaded_apartment(|| Ok(7)).unwrap();
        assert_eq!(value, 7);
        let error = run_in_multithreaded_apartment::<()>(|| Err("boom".into())).unwrap_err();
        assert_eq!(error, "boom");
    }
}
