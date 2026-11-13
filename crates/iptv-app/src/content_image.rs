//! Provider artwork crosses the same DNS/redirect boundary as other content.
//! NSH receives bytes as a data URI, never an untrusted address to fetch itself.
use base64::Engine;
use futures_channel::oneshot;
use nsh_core::prelude::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::Duration,
};

const IMAGE_BYTES: u64 = 4 << 20;
const CACHE_BYTES: usize = 8 << 20;
type Reply = oneshot::Sender<String>;
type Request = (String, Reply);
#[derive(Default)]
struct Cache {
    entries: VecDeque<(String, String)>,
    bytes: usize,
}
impl Cache {
    fn get(&self, url: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|(key, _)| key == url)
            .map(|(_, value)| value.clone())
    }
    fn insert(&mut self, url: String, value: String) {
        let size = url.len() + value.len();
        while self.bytes + size > CACHE_BYTES || self.entries.len() >= 64 {
            let Some((key, value)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= key.len() + value.len();
        }
        self.bytes += size;
        self.entries.push_back((url, value));
    }
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}
fn sender() -> &'static mpsc::SyncSender<Request> {
    static SENDER: OnceLock<mpsc::SyncSender<Request>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Request>(64);
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..4 {
            let rx = rx.clone();
            std::thread::spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                let config = iptv_net::NetConfig {
                    max_streamed_bytes: IMAGE_BYTES,
                    max_buffered_bytes: IMAGE_BYTES as usize,
                    request_timeout: Duration::from_secs(10),
                    allow_files: false,
                    #[cfg(debug_assertions)]
                    allow_private_fixture: cfg!(feature = "test-support")
                        || crate::data::DEV_PLAYLIST.is_some(),
                    ..Default::default()
                };
                let net = runtime.block_on(async { iptv_net::NetClient::new(config) });
                let Ok(net) = net else { return };
                loop {
                    let Ok(request) = rx.lock().expect("image request lock").recv() else {
                        break;
                    };
                    let (url, reply) = request;
                    if reply.is_canceled() {
                        continue;
                    }
                    let cached = cache().lock().expect("image cache lock").get(&url);
                    let value = cached.unwrap_or_else(|| {
                        let value = runtime
                            .block_on(download(&net, &url))
                            .unwrap_or_else(crate::grid::no_logo);
                        cache()
                            .lock()
                            .expect("image cache lock")
                            .insert(url, value.clone());
                        value
                    });
                    let _ = reply.send(value);
                }
            });
        }
        tx
    })
}
async fn download(net: &iptv_net::NetClient, url: &str) -> Option<String> {
    let mut request = iptv_providers::HttpRequest::get(url);
    request.public_only = true;
    let response = net.fetch(request).await.ok()?;
    if !response.is_success() {
        return None;
    }
    // All native SVG paths use nsh_core::svg: its resolver resolves neither
    // external URLs/files nor embedded image hrefs. Data URIs preserve SVG.
    Some(format!(
        "data:image/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(response.body)
    ))
}
/// A recycled view changes its URL reactively; a late response must never
/// overwrite the new channel's artwork. Queueing and network work are bounded.
pub(crate) fn source(url: impl Fn() -> String + 'static) -> impl Fn() -> String {
    let shown = signal(crate::grid::no_logo());
    let generation = signal(0u64);
    effect(move || {
        let url = url();
        let current = generation.get_untracked().wrapping_add(1);
        generation.set(current);
        let cached = cache().lock().expect("image cache lock").get(&url);
        if let Some(value) = cached {
            shown.set(value);
            return;
        }
        shown.set(crate::grid::no_logo());
        if url.len() > 16 * 1024 || !(url.starts_with("http://") || url.starts_with("https://")) {
            return;
        }
        let (tx, rx) = oneshot::channel();
        drop(spawn(async move {
            let mut request = (url, tx);
            loop {
                if generation.get_untracked() != current {
                    return;
                }
                match sender().try_send(request) {
                    Ok(()) => break,
                    Err(mpsc::TrySendError::Full(queued)) => {
                        request = queued;
                        timer(Duration::from_millis(100)).await;
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                }
            }
            if let Ok(value) = rx.await
                && generation.get_untracked() == current
            {
                shown.set(value);
            }
        }));
    });
    move || shown.get()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_has_byte_and_entry_bounds() {
        let mut c = Cache::default();
        for n in 0..100 {
            c.insert(n.to_string(), "x".repeat(200_000));
        }
        assert!(c.bytes <= CACHE_BYTES);
        assert!(c.entries.len() <= 64);
        assert!(c.get("99").is_some());
        assert!(c.get("0").is_none());
    }
    #[test]
    fn private_and_file_images_are_refused_before_connection() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let net = iptv_net::NetClient::new(Default::default()).unwrap();
            for url in [
                "file:///etc/hosts",
                "http://127.0.0.1:9/logo",
                "http://[::1]:9/logo",
            ] {
                assert!(download(&net, url).await.is_none());
            }
        });
    }
    #[test]
    fn fetched_png_and_svg_reach_native_decoder_as_identical_bytes() {
        use std::io::{Read, Write};
        let png=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jZfkAAAAASUVORK5CYII=").unwrap();
        let svg=br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="red"/></svg>"#.to_vec();
        let expected = [png, svg];
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let bodies = expected.clone();
        let server = std::thread::spawn(move || {
            for body in bodies {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = socket.read(&mut request).unwrap();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                socket.write_all(&body).unwrap();
            }
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let config = iptv_net::NetConfig {
                allow_private_fixture: true,
                ..Default::default()
            };
            let net = iptv_net::NetClient::new(config).unwrap();
            for (index, bytes) in expected.iter().enumerate() {
                let uri = download(&net, &format!("http://{address}/{index}"))
                    .await
                    .unwrap();
                assert!(uri.starts_with("data:"));
                let (_, decoded) = nsh_core::image::decode_data_uri(&uri).unwrap();
                assert_eq!(&decoded, bytes);
                if index == 1 {
                    assert!(nsh_core::svg::sniff(&decoded));
                    assert!(nsh_core::svg::rasterize(&decoded, 2).is_ok());
                } else {
                    assert!(decoded.starts_with(b"\x89PNG\r\n\x1a\n"));
                }
            }
        });
        server.join().unwrap();
    }
}
