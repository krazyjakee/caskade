//! Spike: what a WebKitGTK game window can capture, and whether a bundle can be cross-origin isolated.
use std::{borrow::Cow, io::{Read, Write}, time::{Duration, Instant}};
use tao::{
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::unix::WindowExtUnix,
    window::WindowBuilder,
};
use wry::{http::Response, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

const PAGE: &str = include_str!("isolation.html");

enum Ev {
    Title(String),
}

/// A one-page HTTP server on loopback; `headers` adds COOP/COEP.
fn serve(headers: bool) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let extra = if headers {
                "Cross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\n"
            } else {
                ""
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
        }
    });
    port
}

/// A MITM proxy on loopback that logs every response with its full body
/// size and adds COOP/COEP; returns its port.
fn mitm() -> u16 {
    use hudsucker::{
        certificate_authority::RcgenAuthority,
        hyper::{Request, Response},
        rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair},
        rustls::crypto::aws_lc_rs,
        Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
    };
    use http_body_util::BodyExt;

    #[derive(Clone)]
    struct Log;
    impl HttpHandler for Log {
        async fn handle_request(&mut self, _: &HttpContext, req: Request<Body>) -> RequestOrResponse {
            println!("PROXY-REQ {} {}", req.method(), req.uri());
            req.into()
        }
        async fn handle_response(&mut self, ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
            let (mut parts, body) = res.into_parts();
            let bytes = body.collect().await.map(|b| b.to_bytes()).unwrap_or_default();
            println!("PROXY-RES {} {} {}", parts.status, ctx.client_addr, bytes.len());
            parts.headers.insert("cross-origin-opener-policy", "same-origin".parse().unwrap());
            parts.headers.insert("cross-origin-embedder-policy", "require-corp".parse().unwrap());
            Response::from_parts(parts, Body::from(http_body_util::Full::new(bytes)))
        }
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::default();
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            let issuer = Issuer::new(params, key);
            let ca = RcgenAuthority::new(issuer, 1_000, aws_lc_rs::default_provider());
            let proxy = Proxy::builder()
                .with_addr(std::net::SocketAddr::from(([127, 0, 0, 1], port)))
                .with_ca(ca)
                .with_rustls_connector(aws_lc_rs::default_provider())
                .with_http_handler(Log)
                .build()
                .unwrap();
            proxy.start().await.unwrap();
        });
    });
    std::thread::sleep(Duration::from_millis(300));
    port
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].clone();
    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_inner_size(tao::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .unwrap();
    let mut context = WebContext::new(Some(std::env::temp_dir().join(format!("webspike-{mode}"))));
    let url = match mode.as_str() {
        "scheme" | "scheme-noheaders" => "spike://game/index.html".to_owned(),
        "http" => format!("http://127.0.0.1:{}/index.html", serve(true)),
        "http-noheaders" => format!("http://127.0.0.1:{}/index.html", serve(false)),
        "capture" | "proxy" => args[2].clone(),
        other => panic!("unknown mode {other}"),
    };
    let isolate = mode == "scheme";
    let titles = event_loop.create_proxy();
    let mut builder = WebViewBuilder::new_with_web_context(&mut context);
    if mode == "proxy" {
        builder = builder.with_proxy_config(wry::ProxyConfig::Http(wry::ProxyEndpoint {
            host: "127.0.0.1".into(),
            port: mitm().to_string(),
        }));
    }
    let webview = builder
        .with_custom_protocol("spike".into(), move |_, _request| {
            let mut response = Response::builder().header("Content-Type", "text/html; charset=utf-8");
            if isolate {
                response = response
                    .header("Cross-Origin-Opener-Policy", "same-origin")
                    .header("Cross-Origin-Embedder-Policy", "require-corp");
            }
            response.body(Cow::Borrowed(PAGE.as_bytes())).unwrap()
        })
        .with_autoplay(true)
        .with_document_title_changed_handler(move |title| {
            let _ = titles.send_event(Ev::Title(title));
        })
        .build_gtk(window.default_vbox().unwrap())
        .unwrap();
    {
        use webkit2gtk::{WebViewExt, WebsiteDataManagerExt};
        if mode == "proxy" {
            webview
                .webview()
                .website_data_manager()
                .unwrap()
                .set_tls_errors_policy(webkit2gtk::TLSErrorsPolicy::Ignore);
        }
    }
    webview.load_url(&url).unwrap();
    if mode == "capture" || mode == "proxy" {
        use webkit2gtk::{WebResourceExt, WebViewExt};
        webview.webview().connect_resource_load_started(|_, resource, _| {
            println!("STARTED {}", resource.uri().unwrap_or_default());
            resource.connect_finished(|resource| {
                let uri = resource.uri().map(|uri| uri.to_string()).unwrap_or_default();
                resource.data(None::<&gtk::gio::Cancellable>, move |data| match data {
                    Ok(bytes) => println!("CAPTURED {uri} {}", bytes.len()),
                    Err(error) => println!("NODATA {uri} {error}"),
                });
            });
            resource.connect_failed(|resource, error| {
                println!("FAILED {} {error}", resource.uri().unwrap_or_default());
            });
        });
    }
    let deadline = Instant::now() + Duration::from_secs(if mode == "capture" || mode == "proxy" { 10 } else { 6 });
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(deadline);
        let _ = &webview;
        match event {
            Event::UserEvent(Ev::Title(title)) => {
                println!("TITLE {title}");
                if title.starts_with("RESULT") && mode != "proxy" {
                    *flow = ControlFlow::Exit;
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => *flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
