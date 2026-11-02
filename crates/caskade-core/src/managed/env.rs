//! A managed program started from Caskade must not inherit Caskade's own
//! AppImage runtime: an unpacked AppImage's `AppRun` takes `$APPDIR` as
//! its own folder when it is set, so Moonlight's would look for itself in
//! Caskade's mount. Every process Caskade starts gets the same treatment
//! (#274); the rules live in `caskade_installer::runtime::appimage`.

/// Removes Caskade's AppImage runtime from `command`'s environment, keeping
/// the user's own entries and whatever `command` set itself.
pub fn outside_own_appimage(command: &mut std::process::Command) {
    caskade_installer::runtime::outside_appimage(command);
}
