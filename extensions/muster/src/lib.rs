//! `@muster` — supervise units that run in terminals.
//!
//! The parts that are easy to get wrong live here as a host-testable library:
//! unit-file parsing, stack substitution, dotenv merging, and the supervisor's
//! phase and backoff bookkeeping. The protocol plumbing that binds them to a
//! running server is in `main.rs`, which is what compiles to wasm.

pub mod config;
pub mod envfile;
pub mod journal;
pub mod supervisor;
pub mod worktrees;

/// Canonical text form for an opaque YAS resource handle.
///
/// JSON has no lossless `u64` number type. Muster therefore carries handles
/// as fixed-width lowercase hexadecimal strings, without a prefix.
pub fn format_opaque_handle(handle: u64) -> String {
    format!("{handle:016x}")
}

/// Human-facing Terminal CLI form for a terminal handle.
///
/// Core `yas terminal` commands accept decimal identifiers, so text intended
/// to be copied into those commands must not use Muster's hexadecimal wire
/// representation.
pub fn display_terminal_handle(handle: u64) -> String {
    handle.to_string()
}

#[cfg(test)]
mod handle_tests {
    use super::{display_terminal_handle, format_opaque_handle};

    #[test]
    fn opaque_handles_are_fixed_width_lowercase_hex_without_a_prefix() {
        assert_eq!(format_opaque_handle(1), "0000000000000001");
        assert_eq!(
            format_opaque_handle(0x0123_abcd_ef45_6789),
            "0123abcdef456789"
        );
        assert_eq!(format_opaque_handle(u64::MAX), "ffffffffffffffff");
    }

    #[test]
    fn terminal_handles_are_displayed_as_core_cli_decimal_ids() {
        assert_eq!(display_terminal_handle(1), "1");
        assert_eq!(display_terminal_handle(31), "31");
        assert_eq!(display_terminal_handle(u64::MAX), u64::MAX.to_string());
    }
}
