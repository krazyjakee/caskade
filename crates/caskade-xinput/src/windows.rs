//! The foreign calls.

#![allow(unsafe_code)]

use std::ffi::c_void;

#[repr(C)]
#[derive(Default)]
struct Gamepad {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    left_x: i16,
    left_y: i16,
    right_x: i16,
    right_y: i16,
}

#[repr(C)]
#[derive(Default)]
struct State {
    packet: u32,
    gamepad: Gamepad,
}

type GetStateEx = unsafe extern "system" fn(u32, *mut State) -> u32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
}

/// `XInputGetStateEx`, which has only an ordinal.
const ORDINAL: usize = 100;
const SUCCESS: u32 = 0;

/// Reads pads' buttons.
pub struct Reader {
    get_state: GetStateEx,
}

impl Reader {
    /// `None` where Windows has no such call.
    pub fn new() -> Option<Self> {
        for name in [c"xinput1_4.dll", c"xinput1_3.dll"] {
            // SAFETY: the name is a nul-terminated string, and the library
            // stays loaded for the rest of the process, as nothing frees it.
            let library = unsafe { LoadLibraryA(name.as_ptr().cast()) };
            if library.is_null() {
                continue;
            }
            // SAFETY: an ordinal in a pointer's low word is how Windows asks
            // for one; `library` was just loaded.
            let found = unsafe { GetProcAddress(library, ORDINAL as *const u8) };
            if !found.is_null() {
                // SAFETY: ordinal 100 of these libraries has this signature.
                let get_state = unsafe { std::mem::transmute::<*const c_void, GetStateEx>(found) };
                return Some(Self { get_state });
            }
        }
        None
    }

    /// Pad `index`'s buttons; `None` when no pad is there.
    pub fn buttons(&self, index: u32) -> Option<u16> {
        let mut state = State::default();
        // SAFETY: `state` is a valid out pointer of the layout the call writes.
        let result = unsafe { (self.get_state)(index, &mut state) };
        (result == SUCCESS).then_some(state.gamepad.buttons)
    }
}

/// Whether the focused window is this process's.
pub fn focused() -> bool {
    let mut process = 0;
    // SAFETY: a null window is answered with no process, and `process` is a
    // valid out pointer.
    unsafe {
        let window = GetForegroundWindow();
        if !window.is_null() {
            GetWindowThreadProcessId(window, &mut process);
        }
        process == GetCurrentProcessId()
    }
}
