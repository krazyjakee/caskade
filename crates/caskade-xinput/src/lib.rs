//! Windows' XInput, for the guide button while another program has focus.
//!
//! gilrs reads pads through Windows Gaming Input, which sends nothing to a
//! process without the focused window, so a pad couldn't summon Caskade over
//! a game or a stream. XInput answers whoever asks. Its documented call leaves
//! out the guide button, so this uses `XInputGetStateEx`, xinput1_4.dll's
//! ordinal 100, which Windows has kept since Vista. The calls are `unsafe`
//! and stay here, out of crates whose lints forbid them.
//!
//! Windows only, but for [`wants_guide`].

#![deny(unsafe_code)]

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{Reader, focused};

/// How many pads XInput has room for.
pub const PADS: u32 = 4;

const BACK: u16 = 0x0020;
const START: u16 = 0x0010;
const GUIDE: u16 = 0x0400;

/// Whether `buttons` ask for Caskade: the guide button, or Select and Start
/// together, as on pads whose guide button something else keeps.
pub fn wants_guide(buttons: u16) -> bool {
    buttons & GUIDE != 0 || buttons & (BACK | START) == BACK | START
}

#[cfg(test)]
mod tests {
    use super::wants_guide;

    #[test]
    fn guide_or_both_select_and_start_ask_for_caskade() {
        assert!(wants_guide(0x0400));
        assert!(wants_guide(0x0030));
        assert!(wants_guide(0x1030));
        assert!(!wants_guide(0x0010));
        assert!(!wants_guide(0x0020));
        assert!(!wants_guide(0x1000));
        assert!(!wants_guide(0));
    }
}
