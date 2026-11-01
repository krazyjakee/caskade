//! Cache-only artwork protocol. Parsing and file confinement live in core.
//! Without a cache (its folder couldn't be created) every asset is a 404,
//! and the UI draws its placeholders.
use caskade_core::artwork::{ArtworkCache, serve};
use tauri::http::{Method, Response, header};

pub fn register<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    cache: impl Into<Option<ArtworkCache>>,
) -> tauri::Builder<R> {
    let cache = cache.into();
    builder.register_asynchronous_uri_scheme_protocol(
        "caskade-asset",
        move |_context, request, responder| {
            let cache = cache.clone();
            tauri::async_runtime::spawn(async move {
                let response = match &cache {
                    Some(cache) => {
                        response(cache, request.method(), &request.uri().to_string()).await
                    }
                    None => not_found(),
                };
                responder.respond(response);
            });
        },
    )
}

fn not_found() -> Response<Vec<u8>> {
    let mut response = Response::new(Vec::new());
    *response.status_mut() = tauri::http::StatusCode::NOT_FOUND;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn response(cache: &ArtworkCache, method: &Method, uri: &str) -> Response<Vec<u8>> {
    let response = if method != Method::GET {
        Response::builder()
            .status(405)
            .header(header::ALLOW, "GET")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Vec::new())
    } else {
        let asset = serve(cache, uri).await;
        Response::builder()
            .status(asset.status)
            .header(header::CONTENT_TYPE, asset.mime)
            .header(header::CACHE_CONTROL, asset.cache_control)
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(asset.bytes)
    };
    response.unwrap_or_else(|_| {
        let mut response = Response::new(Vec::new());
        *response.status_mut() = tauri::http::StatusCode::INTERNAL_SERVER_ERROR;
        response
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheme_response_is_read_only_confined_and_serves_explicit_thumbnails() {
        tauri::async_runtime::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let cache = ArtworkCache::new(root.path()).unwrap();
            let key = "a".repeat(64);
            std::fs::create_dir(root.path().join(&key)).unwrap();
            std::fs::write(
                root.path().join(&key).join("thumb-320.webp"),
                b"cached thumbnail",
            )
            .unwrap();
            let uri = format!("caskade-asset://localhost/{key}%2Fthumb-320.webp");
            let hit = response(&cache, &Method::GET, &uri).await;
            assert_eq!(hit.status(), 200);
            assert_eq!(hit.headers()[header::CONTENT_TYPE], "image/webp");
            assert_eq!(hit.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(hit.body(), b"cached thumbnail");
            assert_eq!(
                response(
                    &cache,
                    &Method::GET,
                    "caskade-asset://localhost/%2e%2e%2fprivate"
                )
                .await
                .status(),
                400
            );
            assert_eq!(
                response(
                    &cache,
                    &Method::GET,
                    &format!("caskade-asset://localhost/{key}/entry.json")
                )
                .await
                .status(),
                404
            );
            for method in [Method::POST, Method::PUT, Method::DELETE, Method::HEAD] {
                let denied = response(&cache, &method, &uri).await;
                assert_eq!(denied.status(), 405);
                assert_eq!(denied.headers()[header::ALLOW], "GET");
                assert!(denied.body().is_empty());
            }
        });
    }
}
