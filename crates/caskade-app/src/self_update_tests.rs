//! The release feed and its signatures, served here (no network): a newer
//! release is found for this bundle, its download verifies, and a bundle
//! signed by another key, or for another version, is refused.
use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Signer {
    key: minisign::KeyPair,
}

impl Signer {
    fn new() -> Self {
        Self {
            key: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
        }
    }

    /// The public key as the Tauri config has it: base64 of minisign's.
    fn public_key(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.key.pk.to_box().unwrap().to_string())
    }

    /// `bytes`' signature as `tauri signer sign` writes it, for `version`.
    fn sign(&self, bytes: &[u8], version: &str) -> String {
        let signature = minisign::sign(
            Some(&self.key.pk),
            &self.key.sk,
            bytes,
            Some(&format!(
                "timestamp:1\tfile:caskade.AppImage\tversion:{version}"
            )),
            None,
        )
        .unwrap();
        base64::engine::general_purpose::STANDARD.encode(signature.to_string())
    }
}

const BUNDLE: &[u8] = b"\x7fELF a new AppImage";

async fn feed(server: &MockServer, version: &str, signature: &str) -> String {
    let url = format!("{}/caskade.AppImage", server.uri());
    let platforms = serde_json::json!({
        "linux-x86_64": {"signature": signature, "url": url},
        "linux-x86_64-appimage": {"signature": signature, "url": url},
        "linux-x86_64-deb": {"signature": signature, "url": url},
        "windows-x86_64": {"signature": signature, "url": url},
        "darwin-aarch64": {"signature": signature, "url": url},
        "darwin-x86_64": {"signature": signature, "url": url},
    });
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": version,
            "notes": "Better",
            "pub_date": "2026-11-10T00:00:00Z",
            "platforms": platforms,
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/caskade.AppImage"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BUNDLE))
        .mount(server)
        .await;
    format!("{}/latest.json", server.uri())
}

fn bundle() -> Bundle {
    if cfg!(windows) {
        Bundle::Nsis
    } else if cfg!(target_os = "macos") {
        Bundle::App
    } else {
        Bundle::AppImage
    }
}

#[test]
fn a_newer_signed_release_is_found_and_its_download_verifies() {
    crate::runtime::block_on(async {
        let server = MockServer::start().await;
        let signer = Signer::new();
        let endpoint = feed(&server, "0.2.0", &signer.sign(BUNDLE, "0.2.0")).await;
        let release = find(&endpoint, "0.1.4", bundle()).await.unwrap().unwrap();
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.notes.as_deref(), Some("Better"));
        let mut seen = 0;
        let bytes = download(
            &release,
            &signer.public_key(),
            Duration::from_secs(10),
            |got, _| seen = got,
        )
        .await
        .unwrap();
        assert_eq!(bytes, BUNDLE);
        assert_eq!(seen, BUNDLE.len() as u64);
        // Not newer: nothing to install.
        assert_eq!(find(&endpoint, "0.2.0", bundle()).await.unwrap(), None);
        assert_eq!(find(&endpoint, "v0.3.0", bundle()).await.unwrap(), None);
    });
}

#[test]
fn another_keys_or_another_versions_signature_is_refused() {
    crate::runtime::block_on(async {
        let server = MockServer::start().await;
        let (signer, stranger) = (Signer::new(), Signer::new());
        let endpoint = feed(&server, "0.2.0", &stranger.sign(BUNDLE, "0.2.0")).await;
        let release = find(&endpoint, "0.1.4", bundle()).await.unwrap().unwrap();
        let error = download(
            &release,
            &signer.public_key(),
            Duration::from_secs(10),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(error.contains("signature"), "{error}");
        // A genuine signature for an older version, replayed.
        let replayed = signer.sign(BUNDLE, "0.1.0");
        let error = verify(BUNDLE, &replayed, &signer.public_key(), "0.2.0").unwrap_err();
        assert!(error.contains("0.1.0"), "{error}");
        // And the bytes must be the signed ones.
        let error = verify(
            b"other bytes",
            &signer.sign(BUNDLE, "0.2.0"),
            &signer.public_key(),
            "0.2.0",
        )
        .unwrap_err();
        assert!(error.contains("signature"), "{error}");
    });
}

#[test]
fn the_shipped_key_is_readable() {
    let key = public_key().expect("the updater key in caskade-app/updater.pub");
    assert!(minisign_verify::PublicKey::decode(&base64_text(&key).unwrap()).is_ok());
}

#[test]
fn a_deb_takes_only_its_own_entry() {
    assert_eq!(Bundle::Deb.platforms().len(), 1);
    assert!(Bundle::Deb.platforms()[0].ends_with("-deb"));
    assert_eq!(Bundle::AppImage.platforms().len(), 2);
}

#[test]
fn only_caskades_own_appimage_counts() {
    let env = |appdir: &'static str| {
        move |name: &str| match name {
            "APPIMAGE" => Some(OsString::from("/home/p/Launcher.AppImage")),
            "APPDIR" => Some(OsString::from(appdir)),
            _ => None,
        }
    };
    // A deb started from another AppImage inherits its variables.
    let deb = Path::new("/usr/bin/caskade");
    assert_eq!(appimage(deb, env("/tmp/.mount_LauncherAbc")), None);
    let mounted = Path::new("/tmp/.mount_CaskadXyz/usr/bin/caskade");
    assert_eq!(
        appimage(mounted, env("/tmp/.mount_CaskadXyz")),
        Some(PathBuf::from("/home/p/Launcher.AppImage"))
    );
    if cfg!(target_os = "linux") {
        assert_eq!(
            Bundle::of(deb, env("/tmp/.mount_LauncherAbc")),
            Some(Bundle::Deb)
        );
        assert_eq!(
            Bundle::of(mounted, env("/tmp/.mount_CaskadXyz")),
            Some(Bundle::AppImage)
        );
    }
}

#[cfg(unix)]
#[test]
fn an_appimage_is_replaced_in_place_and_keeps_running_mode() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let appimage = temp.path().join("Caskade.AppImage");
    std::fs::write(&appimage, b"\x7fELF the old one").unwrap();
    std::fs::set_permissions(&appimage, std::fs::Permissions::from_mode(0o750)).unwrap();
    replace_appimage(&appimage, BUNDLE).unwrap();
    assert_eq!(std::fs::read(&appimage).unwrap(), BUNDLE);
    let mode = std::fs::metadata(&appimage).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o751);
    // Not an AppImage: the old one stays.
    assert!(replace_appimage(&appimage, b"<html>").is_err());
    assert_eq!(std::fs::read(&appimage).unwrap(), BUNDLE);
}
