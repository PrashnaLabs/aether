use super::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct ApiFixture {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl ApiFixture {
    fn new(routes: HashMap<String, (u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let server = std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("accept fixture request: {e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                        Err(_) => {
                            if done.load(Ordering::SeqCst) {
                                return;
                            }
                            break;
                        }
                    }
                }
                if !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    if done.load(Ordering::SeqCst) {
                        return;
                    }
                    continue;
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap().to_string();
                log.lock().unwrap().push(path.clone());
                let (status, body) = routes.get(&path).cloned().unwrap_or((404, "null".into()));
                let _ = write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        Self { base, requests, stop, server: Some(server) }
    }

    fn page(&self, page: usize, ids: Vec<u64>) -> Result<StoryPage, String> {
        // The actual HTTP future is polled outside Tokio, like GPUI.
        futures::executor::block_on(fetch_page(self.base.clone(), HnCategory::Top, page, ids))
    }
}

impl Drop for ApiFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

#[test]
fn official_api_paginates_in_order_without_tokio_context() {
    let ids: Vec<u64> = (1..=31).collect();
    let mut routes =
        HashMap::from([("/topstories.json".into(), (200, serde_json::to_string(&ids).unwrap()))]);
    for id in &ids {
        let body = match id {
            2 => r#"{"id":2,"deleted":true}"#.into(),
            3 => "null".into(),
            4 => r#"{"id":4,"dead":true}"#.into(),
            _ => format!(
                r#"{{"id":{id},"type":"story","title":"Story {id}","by":"author","score":12,"time":123,"descendants":7}}"#
            ),
        };
        routes.insert(format!("/item/{id}.json"), (200, body));
    }
    let api = ApiFixture::new(routes);
    let first = api.page(0, vec![]).unwrap();
    assert!(first.has_more);
    assert_eq!(
        first.stories.iter().map(|s| s.id).collect::<Vec<_>>(),
        (1..=30).filter(|id| ![2, 3, 4].contains(id)).collect::<Vec<_>>()
    );
    assert_eq!(first.stories[0].comments_count, 7);
    let last = api.page(1, first.ids).unwrap();
    assert!(!last.has_more);
    assert_eq!(last.stories[0].id, 31);
    let requests = api.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|r| *r == "/topstories.json").count(), 1);
    assert_eq!(requests.len(), 32);
}

#[test]
fn empty_feed_is_not_a_network_error() {
    let api = ApiFixture::new(HashMap::from([("/topstories.json".into(), (200, "[]".into()))]));
    let result = api.page(0, vec![]).unwrap();
    assert!(result.stories.is_empty());
    assert!(!result.has_more);
}

#[test]
fn api_errors_are_reported_for_retry() {
    for (status, body) in [(503, "unavailable"), (200, "invalid json")] {
        let api =
            ApiFixture::new(HashMap::from([("/topstories.json".into(), (status, body.into()))]));
        assert!(api.page(0, vec![]).is_err());
    }
}

#[test]
fn duplicate_ids_do_not_repeat_items() {
    let api = ApiFixture::new(HashMap::from([
        ("/topstories.json".into(), (200, "[42,42]".into())),
        ("/item/42.json".into(), (200, r#"{"id":42,"type":"story","title":"Only once"}"#.into())),
    ]));
    let page = api.page(0, vec![]).unwrap();
    assert_eq!(page.ids, vec![42]);
    assert_eq!(page.stories.len(), 1);
    assert!(!page.has_more);
}
