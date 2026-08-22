#![no_std]
#![doc = include_str!("../README.md")]

extern crate alloc;

#[cfg(not(target_arch = "wasm32"))]
extern crate std;

pub mod channel;
pub mod collections;
pub mod command;
pub mod env;
pub mod events;
pub mod fs;
mod host;
pub mod kv;
pub mod net;
pub mod process;
mod receive;
pub mod surface;
#[path = "terminal_native.rs"]
pub mod terminal;
#[cfg(test)]
mod test_support;
pub mod transfer;
pub mod yas;

#[cfg(not(target_arch = "wasm32"))]
pub mod native_host;

pub use host::{WaitOutcome, random as fill_random};
pub use yas::{Client, EXIT_BOOTSTRAP_FAILURE, Error, MonotonicInstant, Realtime};

#[doc(hidden)]
pub use getrandom::register_custom_getrandom as __register_getrandom_02;

/// Result types accepted by [`entry!`].
pub trait EntryResult {
    /// The exit code, and what to say about it.
    ///
    /// The message half exists because dropping it is how an extension came to
    /// exit within two seconds of every start while `ext status` reported
    /// `code=1, detail=""`: the reason existed, in a variable, and nothing ever
    /// read it. This crate is `no_std` and cannot print, so it hands the string
    /// back and `entry!` — which expands inside the extension — writes it.
    fn finish(self) -> (i32, Option<alloc::string::String>);
}

impl EntryResult for () {
    fn finish(self) -> (i32, Option<alloc::string::String>) {
        (0, None)
    }
}

impl EntryResult for i32 {
    fn finish(self) -> (i32, Option<alloc::string::String>) {
        (self, None)
    }
}

impl<E: core::fmt::Display> EntryResult for Result<(), E> {
    fn finish(self) -> (i32, Option<alloc::string::String>) {
        match self {
            Ok(()) => (0, None),
            Err(error) => (1, Some(alloc::format!("{error}"))),
        }
    }
}

/// Bootstrap a native YAS client, invoke a guest function, and return its exit
/// code.
///
/// Most guests use [`entry!`] rather than calling this directly.
pub fn run_entry<F, R>(entry: F) -> (i32, Option<alloc::string::String>)
where
    F: FnOnce(Client) -> R,
    R: EntryResult,
{
    match yas::Client::bootstrap() {
        Ok(client) => entry(client).finish(),
        Err(_) => (
            EXIT_BOOTSTRAP_FAILURE,
            Some(alloc::string::String::from(
                "could not bootstrap the YAS client",
            )),
        ),
    }
}

/// Fill a buffer for the pinned `getrandom` 0.2 custom backend.
#[doc(hidden)]
pub fn __getrandom_v02(bytes: &mut [u8]) -> Result<(), getrandom::Error> {
    host::random(bytes).map_err(|_| {
        let code = core::num::NonZeroU32::new(getrandom::Error::CUSTOM_START + 1)
            .expect("custom getrandom code is non-zero");
        getrandom::Error::from(code)
    })
}

/// Install YAS's entropy source as the `getrandom` 0.2 custom backend.
///
/// Expand this once in the root guest crate if it does not use [`entry!`].
/// The SDK pins `getrandom` 0.2.17; `rand` 0.8 uses this backend without a JS
/// adapter. Newer `getrandom` major versions use a different selection model.
#[macro_export]
macro_rules! register_getrandom {
    () => {
        $crate::__register_getrandom_02!($crate::__getrandom_v02);
    };
}

/// Export a native YAS function as the Wasmi contract's
/// `yas_main: () -> i32`.
///
/// The function receives a fully bootstrapped [`Client`]. It may return `()`,
/// an `i32`, or `Result<(), E>`. Bootstrap failure exits with
/// [`EXIT_BOOTSTRAP_FAILURE`].
///
/// ```ignore
/// fn extension(mut client: yas_guest::Client) -> Result<(), yas_guest::Error> {
///     let _ = client.ping()?;
///     Ok(())
/// }
///
/// yas_guest::entry!(extension);
/// ```
#[macro_export]
macro_rules! entry {
    ($entry:path) => {
        $crate::register_getrandom!();

        #[doc(hidden)]
        #[unsafe(export_name = "yas_wire_v1")]
        pub extern "C" fn __yas_guest_yas_v1() -> i32 {
            1
        }

        #[unsafe(export_name = "yas_main")]
        pub extern "C" fn __yas_guest_main() -> i32 {
            // Expanded in the extension's own crate, which has `std`: this is
            // the one place that can turn the entry's error into output the
            // attempt retains.
            let (code, message) = $crate::run_entry($entry);
            if let Some(message) = message {
                eprintln!("{message}");
            }
            code
        }
    };
}
