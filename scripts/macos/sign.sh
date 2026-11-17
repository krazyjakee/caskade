#!/usr/bin/env bash
# Signs a macOS release of Caskade with its Developer ID and the hardened
# runtime, and with --notarize has Apple notarize it and staples the ticket.
#
#   scripts/macos/sign.sh [--notarize] Caskade_<version>_<arch>.app.tar.gz [Caskade_<version>_<arch>.dmg]
#
# The updater tarball's app is signed (7zz with it), checked, notarized and
# stapled, then packed back under the same name. Given the dmg as well, the
# signed app replaces the one inside it, and the dmg is signed, notarized and
# stapled too. Both files are replaced in place. Signing and notarizing run
# anywhere rcodesign does; the dmg and the Gatekeeper checks need macOS
# (hdiutil, codesign, spctl, stapler). docs/RELEASING.md has the one-time setup.
#
# The identity: MACOS_SIGNING_PEM (a file: private key, Developer ID
# Application certificate, its CA, Apple's root) or MACOS_SIGNING_PEM_B64 (its
# base64). Notarizing: MACOS_NOTARY_KEY (rcodesign's App Store Connect API key
# JSON) or MACOS_NOTARY_KEY_B64. RCODESIGN names the rcodesign to run.
set -euo pipefail

here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo="$(dirname -- "$(dirname -- "$here")")"
entitlements="$here/entitlements.plist"
rcodesign="${RCODESIGN:-rcodesign}"

notarize=0
if [[ "${1:-}" == --notarize ]]; then
	notarize=1
	shift
fi
if [[ $# -lt 1 || $# -gt 2 || "$1" != *.app.tar.gz || ($# -eq 2 && "$2" != *.dmg) ]]; then
	sed -n '2,/^set -/{/^set -/d;s/^# \{0,1\}//;p}' "$0" >&2
	exit 2
fi
tarball="$1"
dmg="${2:-}"
on_macos=0
[[ "$(uname -s)" == Darwin ]] && on_macos=1
if [[ -n "$dmg" ]] && ((!on_macos)); then
	echo "Signing the dmg needs macOS (hdiutil)." >&2
	exit 1
fi
command -v "$rcodesign" >/dev/null || {
	echo "rcodesign not found: install it or set RCODESIGN." >&2
	exit 1
}

work="$(mktemp -d)"
chmod 700 "$work"
mounted=""
cleanup() {
	[[ -n "$mounted" ]] && hdiutil detach -force "$mounted" >/dev/null 2>&1
	rm -rf "$work"
}
trap cleanup EXIT

# The secret as a 0600 file in the work folder, from a path or from base64.
materialise() {
	local path="${!1:-}" encoded="${!2:-}" destination="$3"
	if [[ -n "$path" ]]; then
		[[ -s "$path" ]] || {
			echo "$1 is set to $path, which is missing or empty." >&2
			exit 1
		}
		(umask 077 && cat -- "$path" >"$destination")
	elif [[ -n "$encoded" ]]; then
		(umask 077 && printf '%s' "$encoded" | base64 --decode >"$destination")
	else
		echo "Set $1 or $2." >&2
		exit 1
	fi
}
identity="$work/identity.pem"
materialise MACOS_SIGNING_PEM MACOS_SIGNING_PEM_B64 "$identity"
notary_key="$work/notary.json"
((notarize)) && materialise MACOS_NOTARY_KEY MACOS_NOTARY_KEY_B64 "$notary_key"

notarize_and_staple() {
	echo "Notarizing $(basename "$1"); Apple takes a few minutes."
	# --staple waits for Apple's answer, then writes the ticket into the file,
	# so the first start needs no network to pass Gatekeeper.
	"$rcodesign" notary-submit --api-key-file "$notary_key" --staple "$1"
}

# rcodesign exits 0 on results that aren't what was asked for, such as an
# ad-hoc signature from a PEM without its private key: read each one back.
check_signature() {
	local info="$work/signature.txt"
	"$rcodesign" print-signature-info "$1" >"$info"
	if grep -q ADHOC "$info"; then
		echo "$1 is still ad-hoc signed." >&2
		exit 1
	fi
	grep -q RUNTIME "$info" || {
		echo "$1 doesn't have the hardened runtime." >&2
		exit 1
	}
	grep -q "CN=Developer ID Application: " "$info" || {
		echo "$1 isn't signed with a Developer ID Application certificate." >&2
		exit 1
	}
}

# The app. The tarball holds Caskade.app at its root, as the bundler made it.
mkdir "$work/app"
tar -xzf "$tarball" -C "$work/app"
app="$work/app/Caskade.app"
[[ -d "$app" ]] || {
	echo "$tarball has no Caskade.app at its root." >&2
	exit 1
}

echo "Signing $(basename "$tarball")"
# --for-notarization turns on the hardened runtime, requires a timestamp and
# refuses a certificate notarization would, so a wrong identity fails here.
# The entitlements are the main executable's; 7zz, signed on the way through
# the bundle, gets none.
"$rcodesign" sign --for-notarization \
	--pem-file "$identity" \
	--entitlements-xml-file "$entitlements" \
	"$app"
for program in "$app"/Contents/MacOS/*; do
	check_signature "$program"
done
signer="$("$rcodesign" print-signature-info "$app/Contents/MacOS/caskade" |
	grep -m1 -o "CN=Developer ID Application: [^,']*")"
echo "Signed with ${signer#CN=}"

if ((on_macos)); then
	codesign --verify --deep --strict "$app"
	# The app runs its 7zz only if it meets the requirement in the source
	# (SevenZip::pinned): check this one does before it ships.
	requirement="$(sed -nE 's/^ *r#"(anchor apple generic and .*)"#;$/\1/p' \
		"$repo/crates/caskade-installer/src/formats/archive/sevenzip/mod.rs")"
	[[ -n "$requirement" ]] || {
		echo "MACOS_RELEASE_REQUIREMENT not found in the 7-Zip source." >&2
		exit 1
	}
	codesign --verify --strict -R "=$requirement" "$app/Contents/MacOS/7zz"
	echo "7zz meets: $requirement"
fi

if ((notarize)); then
	notarize_and_staple "$app"
	if ((on_macos)); then
		xcrun stapler validate "$app"
		spctl --assess --type execute --verbose=2 "$app"
	fi
fi

# Packed back without macOS's metadata files, so the tarball holds the bundle
# and nothing else.
COPYFILE_DISABLE=1 tar -czf "$work/app.tar.gz" -C "$work/app" Caskade.app
mv -f "$work/app.tar.gz" "$tarball"
echo "Repacked $tarball"

[[ -z "$dmg" ]] && exit 0

# The dmg: swap the signed app into the bundler's image, which keeps its
# window layout, then sign the image itself.
echo "Signing $(basename "$dmg")"
hdiutil convert "$dmg" -quiet -format UDRW -o "$work/rw.dmg"
# Room for the signatures and the ticket on top of the bundler's margin.
hdiutil resize -quiet -size "$(($(stat -f%z "$work/rw.dmg") / 1048576 + 64))m" "$work/rw.dmg"
mkdir "$work/mount"
hdiutil attach -quiet -nobrowse -noautoopen -noverify -mountpoint "$work/mount" "$work/rw.dmg"
mounted="$work/mount"
[[ -d "$mounted/Caskade.app" ]] || {
	echo "$dmg has no Caskade.app at its root." >&2
	exit 1
}
rm -rf "$mounted/Caskade.app"
ditto "$app" "$mounted/Caskade.app"
codesign --verify --deep --strict "$mounted/Caskade.app"
hdiutil detach -quiet "$mounted"
mounted=""
hdiutil convert "$work/rw.dmg" -quiet -format UDZO -imagekey zlib-level=9 -o "$work/signed.dmg"
"$rcodesign" sign --for-notarization --pem-file "$identity" "$work/signed.dmg"
codesign --verify --strict "$work/signed.dmg"
if ((notarize)); then
	notarize_and_staple "$work/signed.dmg"
	xcrun stapler validate "$work/signed.dmg"
	spctl --assess --type open --context context:primary-signature --verbose=2 "$work/signed.dmg"
fi
mv -f "$work/signed.dmg" "$dmg"
echo "Replaced $dmg"
