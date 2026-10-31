use super::*;
#[cfg(target_os = "linux")]
use gtk::prelude::*;
use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
#[cfg(target_os = "linux")]
use webkit2gtk::{SettingsExt, WebViewExt};
use wry::{PermissionKind, PermissionResponse, WebViewBuilder};
#[cfg(target_os = "linux")]
use wry::{WebViewBuilderExtUnix, WebViewExtUnix};
pub(super) fn emit(value: Value) {
    if let Ok(bytes) = serde_json::to_vec(&value) {
        let mut output = std::io::stdout().lock();
        let _ = output
            .write_all(&bytes)
            .and_then(|_| output.write_all(b"\n"))
            .and_then(|_| output.flush());
    }
}
pub(super) fn run() -> Result<(), String> {
    let mut input = BufReader::new(std::io::stdin());
    let first = read_frame(&mut input)?.ok_or("No host document was provided")?;
    if first["type"] != "document" {
        return Err("The native host expected an approved document".into());
    }
    let document: Document = serde_json::from_value(first["document"].clone())
        .map_err(|_| "The native host document is invalid")?;
    let server = Server::new(&document)?;
    let origin = server.origin.clone();
    let root_url = server.root_url.clone();
    let event_loop = EventLoopBuilder::<Value>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        loop {
            match read_frame(&mut input) {
                Ok(Some(value)) => {
                    if proxy.send_event(value).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = proxy.send_event(json!({"type":"close"}));
                    break;
                }
            }
        }
    });
    let window = WindowBuilder::new()
        .with_title(document.title)
        .with_inner_size(tao::dpi::LogicalSize::new(1080.0, 760.0))
        .build(&event_loop)
        .map_err(|_| "The native host window could not be created")?;
    let navigate_origin = origin.clone();
    let ipc_origin = origin.clone();
    let media = document.allow_media;
    let builder = WebViewBuilder::new()
        .with_devtools(false)
        .with_navigation_handler(move |url| url.starts_with(&format!("{navigate_origin}/")))
        .with_permission_handler(move |kind| {
            if media && kind == PermissionKind::Microphone {
                PermissionResponse::Allow
            } else {
                PermissionResponse::Deny
            }
        })
        .with_ipc_handler(move |request| {
            if request.uri().to_string().starts_with(&ipc_origin)
                && request.body().len() <= MAX_FRAME
            {
                if let Ok(value) = serde_json::from_str::<Value>(request.body()) {
                    emit(json!({"type":"host-web-message","message":value}));
                }
            }
        });
    #[cfg(target_os = "linux")]
    let webview = builder
        .build_gtk(
            window
                .default_vbox()
                .ok_or("The native host content container is unavailable")?,
        )
        .map_err(|_| "The native host browser could not be created")?;
    #[cfg(not(target_os = "linux"))]
    let webview = builder
        .build(&window)
        .map_err(|_| "The native host browser could not be created")?;
    #[cfg(target_os = "linux")]
    if let Some(settings) = WebViewExt::settings(&webview.webview()) {
        settings.set_enable_media_stream(media);
        settings.set_enable_webrtc(media);
    }
    webview
        .load_url(&root_url)
        .map_err(|_| "The native host document could not be loaded")?;
    emit(json!({"type":"host-ready"}));
    emit(
        json!({"type":"host-window-state","focused":window.is_focused(),"visible":window.is_visible()}),
    );
    let mut closing: Option<(std::time::Instant, &'static str)> = None;
    event_loop.run(move |event, _, control_flow| {
        if matches!(event, Event::LoopDestroyed) { server.stop.store(true, Ordering::Release); return; }
        if let Some((deadline, reason)) = closing {
            if std::time::Instant::now() >= deadline {
                emit(json!({"type":"closed","reason":reason}));
                *control_flow = ControlFlow::Exit;
                return;
            }
            *control_flow = ControlFlow::WaitUntil(deadline);
        } else { *control_flow = ControlFlow::Wait; }
        match event {
            Event::UserEvent(value) if value["type"] == "visibility" && closing.is_none() => {
                if let Some(visible)=value["visible"].as_bool() {window.set_visible(visible);}
                emit(json!({"type":"host-window-state","focused":window.is_focused(),"visible":window.is_visible()}));
            }
            Event::UserEvent(value) if value["type"] == "focus" && closing.is_none() => {
                #[cfg(target_os = "linux")]
                { let gtk_window = window.gtk_window(); gtk_window.show_all(); gtk_window.deiconify(); gtk_window.present(); }
                #[cfg(not(target_os = "linux"))]
                { window.set_minimized(false); window.set_visible(true); window.set_focus(); }
                emit(json!({"type":"host-window-state","focused":window.is_focused(),"visible":window.is_visible()}));
            }
            Event::UserEvent(value) if value["type"] == "close" => {
                if closing.is_none() {
                    let _ = webview.evaluate_script("window.dispatchEvent(new CustomEvent('hermes-host-message',{detail:{type:'close'}}));");
                    let deadline = std::time::Instant::now() + Duration::from_millis(100);
                    closing = Some((deadline, "parent-closed"));
                    *control_flow = ControlFlow::WaitUntil(deadline);
                }
            }
            Event::UserEvent(value) if closing.is_none() => {
                let data = value.to_string();
                let script = format!("window.dispatchEvent(new CustomEvent('hermes-host-message',{{detail:{data}}}));");
                if webview.evaluate_script(&script).is_err() { emit(json!({"type":"error","message":"The host page could not receive a command"})); }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if closing.is_none() {
                    let _ = webview.evaluate_script("window.dispatchEvent(new CustomEvent('hermes-host-message',{detail:{type:'close'}}));");
                    let deadline = std::time::Instant::now() + Duration::from_millis(100);
                    closing = Some((deadline, "window-closed"));
                    *control_flow = ControlFlow::WaitUntil(deadline);
                }
            }
            Event::WindowEvent { .. } => { emit(json!({"type":"host-window-state","focused":window.is_focused(),"visible":window.is_visible()})); }
            Event::LoopDestroyed => { server.stop.store(true, Ordering::Release); }
            _ => {}
        }
    });
}
struct Server {
    origin: String,
    root_url: String,
    stop: Arc<AtomicBool>,
}
impl Server {
    fn new(document: &Document) -> Result<Self, String> {
        if document.html.len() + document.assets.values().map(String::len).sum::<usize>()
            > 64 * 1024 * 1024
        {
            return Err("The approved host document exceeds its size limit".into());
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "The native document port could not be opened")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "The native document server could not be configured")?;
        let port = listener
            .local_addr()
            .map_err(|_| "The native document address is unavailable")?
            .port();
        let origin = format!("http://127.0.0.1:{port}");
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let prefix = format!("/{nonce}/");
        let root_url = format!("{origin}{prefix}index.html");
        let html = document.html.clone();
        let assets = document.assets.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        if peer.ip().is_loopback() {
                            serve(stream, port, &prefix, &html, &assets);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            origin,
            root_url,
            stop,
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
fn serve(
    mut stream: TcpStream,
    port: u16,
    prefix: &str,
    html: &str,
    assets: &BTreeMap<String, String>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut bytes = vec![];
    let mut buffer = [0u8; 1024];
    while bytes.len() < 8192 {
        let Ok(count) = stream.read(&mut buffer) else {
            return;
        };
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let Ok(request) = std::str::from_utf8(&bytes) else {
        return;
    };
    let mut lines = request.split("\r\n");
    let Some(line) = lines.next() else {
        return;
    };
    let parts: Vec<_> = line.split(' ').collect();
    if parts.len() != 3 || parts[0] != "GET" {
        return;
    }
    let hosts: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect();
    if hosts.len() != 1 || hosts[0] != format!("127.0.0.1:{port}") {
        return;
    }
    let target = parts[1].split('?').next().unwrap_or_default();
    let Some(asset) = target.strip_prefix(prefix) else {
        return;
    };
    let (body, mime) = if asset == "index.html" {
        (html, "text/html; charset=utf-8")
    } else if let Some(body) = assets.get(asset) {
        (
            body.as_str(),
            if asset.ends_with(".css") {
                "text/css; charset=utf-8"
            } else {
                "text/javascript; charset=utf-8"
            },
        )
    } else {
        ("Not found", "text/plain")
    };
    let status = if body == "Not found" {
        "404 Not Found"
    } else {
        "200 OK"
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}
