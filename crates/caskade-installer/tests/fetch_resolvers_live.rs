#![cfg(feature = "fetch")]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, time::Duration};

/// Explicitly refresh public metadata only; no release asset is downloaded or run.
#[tokio::test]
#[ignore = "live GitHub metadata; run just test-resolvers-live explicitly"]
async fn refresh_public_github_response() {
    assert_eq!(
        std::env::var("CASKADE_RESOLVERS_LIVE").as_deref(),
        Ok("1"),
        "set CASKADE_RESOLVERS_LIVE=1 to opt into live network recording"
    );
    let source = "https://api.github.com/repos/ip7z/7zip/releases/tags/26.05";
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("CaskadeInstaller-resolver-recording")
        .build()
        .unwrap()
        .get(source)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut response = response;
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(bytes.len() + chunk.len() <= 4 * 1024 * 1024);
        bytes.extend_from_slice(&chunk);
    }
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    let assets = raw["assets"].as_array().unwrap();
    assert!(!assets.is_empty() && assets.len() <= 1024);
    let assets: Vec<_> = assets
        .iter()
        .map(|asset| {
            json!({
                "name": asset["name"],
                "browser_download_url": asset["browser_download_url"],
                "size": asset["size"],
                "state": asset["state"],
                "digest": asset["digest"],
            })
        })
        .collect();
    let projected = json!({
        "tag_name":raw["tag_name"], "draft":raw["draft"],
        "prerelease":raw["prerelease"], "assets":assets,
    });
    let provenance = json!({
        "source_url": source,
        "recorded_unix_seconds": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        "raw_response_sha256": format!("{:x}", Sha256::digest(&bytes)),
        "recording": "Projected public GitHub response; only release flags and download asset metadata retained. Tests replace URLs, sizes and digests for local committed fixture payloads.",
    });
    let dir = std::env::var_os("CASKADE_RESOLVERS_RECORD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fetch-responses"));
    fs::create_dir_all(&dir).unwrap();
    for (name, value) in [
        ("github-7zip-26.05.json", projected),
        ("github-7zip-26.05.provenance.json", provenance),
    ] {
        fs::write(
            dir.join(name),
            format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
        )
        .unwrap();
    }
}
