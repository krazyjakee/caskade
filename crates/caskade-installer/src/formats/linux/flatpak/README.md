# Flatpak delegate

FlatpakProbe identifies genuine flatpak bundle headers and flatpakref keyfiles.
Bundle inspection uses OSTree/GVariant framing offsets to find only ref and
metadata dictionary values. It skips inline compressed delta parts; each text
member is capped at64KiB, dictionary count at10,000, and all reads share the
probe budget. The real app ID must agree between the bundle ref and Application
metadata. Invalid metadata yields a format error fact and no executable plan.
References accept credential-free HTTP(S) URLs and validated IDs/branches.

FlatpakHandler proposes only Linux user delegation when Flatpak is present in
injected host package capabilities. It uses install --user --noninteractive,
with an argument terminator, and records a specific-ref uninstall command.
The runtime resolves the logical flatpak tool. There is no host inspection or
execution while probing/planning.

FlatpakInstaller accepts an executor, the selected artifact/source provenance,
and a normal ExecRequest. Its finalizer records the delegate and inverse command
inside a managed metadata folder with relocation verdict Pinned; the actual app
stays managed by Flatpak outside the storage root. A successful report carries
HandOff::Delegated with a flatpak URI and uninstall steps. FlatpakMetadata also
provides a flatpak run launch specification. Generic extraction discovery is not
used for a delegated app with no files in the managed folder.

The native test uses an injected runtime that sets private FLATPAK_USER_DIR and
XDG data/config/cache paths on every spawned install/uninstall process. It
creates a temporary runtime locally from host ELF dependencies, performs actual
bundle install and manifest uninstall, removes the test runtime, and asserts
that the private installation returns to its initial empty ref list. No global
environment mutation, runtime download, or modification to the real user's
Flatpak installation is required. Native runner acceptance is #22.

Format references: Flatpak common/flatpak-repo-utils.c and
app/flatpak-builtins-build-bundle.c, OSTree static-delta superblock format, and
the gvariant0.5.1 typed parser. Pure Rust parsing is cross-compiled on all hosts;
only Linux receives a native delegate candidate.
