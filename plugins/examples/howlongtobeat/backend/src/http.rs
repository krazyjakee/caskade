//! A JSON POST through `wasi:http`.
use wasi::http::{
    outgoing_handler,
    types::{Fields, Method, OutgoingBody, OutgoingRequest, Scheme},
};
use wasi::io::streams::StreamError;

/// POSTs `body` to `url` (`http(s)://host[:port]/path`); returns the status
/// and the response text.
pub fn post_json(url: &str, body: &str) -> Result<(u16, String), String> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        return Err(format!("not an http(s) URL: {url}"));
    };
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    let headers = Fields::from_list(&[
        ("content-type".to_owned(), b"application/json".to_vec()),
        ("accept".to_owned(), b"application/json".to_vec()),
        ("referer".to_owned(), b"https://howlongtobeat.com".to_vec()),
    ])
    .map_err(|error| format!("{error:?}"))?;
    let request = OutgoingRequest::new(headers);
    request.set_method(&Method::Post).map_err(|()| "method")?;
    request.set_scheme(Some(&scheme)).map_err(|()| "scheme")?;
    request
        .set_authority(Some(authority))
        .map_err(|()| "authority")?;
    request
        .set_path_with_query(Some(&path))
        .map_err(|()| "path")?;
    let outgoing = request.body().map_err(|()| "request body taken")?;
    {
        let stream = outgoing.write().map_err(|()| "request stream taken")?;
        for chunk in body.as_bytes().chunks(4096) {
            stream
                .blocking_write_and_flush(chunk)
                .map_err(|error| format!("{error:?}"))?;
        }
    }
    let pending = outgoing_handler::handle(request, None).map_err(|error| format!("{error:?}"))?;
    OutgoingBody::finish(outgoing, None).map_err(|error| format!("{error:?}"))?;
    pending.subscribe().block();
    let response = pending
        .get()
        .ok_or("no response")?
        .map_err(|()| "response taken twice")?
        .map_err(|error| format!("{error:?}"))?;
    let status = response.status();
    let incoming = response.consume().map_err(|()| "response body taken")?;
    let stream = incoming.stream().map_err(|()| "response stream taken")?;
    let mut bytes = Vec::new();
    loop {
        match stream.blocking_read(64 * 1024) {
            Ok(chunk) => bytes.extend(chunk),
            Err(StreamError::Closed) => break,
            Err(error) => return Err(format!("{error:?}")),
        }
        if bytes.len() > 4 << 20 {
            return Err("response larger than 4 MiB".into());
        }
    }
    String::from_utf8(bytes)
        .map(|text| (status, text))
        .map_err(|_| "response is not UTF-8".into())
}
