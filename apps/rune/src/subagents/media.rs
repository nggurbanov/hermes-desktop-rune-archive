//! Explicit file actions retain the immutable child owner and native destination.
use super::*;
use crate::{gateway::Gateway, media::TempMedia, pages::PageRequest};
use std::path::PathBuf;
struct Pending {
    id: u64,
    owner: DraftScope,
    path: PathBuf,
    open: bool,
}
struct Reply {
    id: u64,
    owner: DraftScope,
    result: Result<PathBuf, String>,
}
pub(crate) struct Files {
    pending: Option<Pending>,
    send: Sender<Reply>,
    receive: Receiver<Reply>,
    next: u64,
    waker: Option<Waker>,
    temp: TempMedia,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub open_url: Option<String>,
}
impl Files {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            pending: None,
            send,
            receive,
            next: 0,
            waker: None,
            temp: TempMedia::new(),
            error: None,
            notice: None,
            open_url: None,
        }
    }
    pub fn init(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn start(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        source: String,
        label: String,
        destination: Option<PathBuf>,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("Wait for the current file transfer to finish".into());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("The file transfer is not initialized")?;
        let open = destination.is_none();
        let path = match destination {
            Some(path) if path.is_absolute() => path,
            Some(_) => return Err("The native save destination must be absolute".into()),
            None => self.temp.reserve(&label)?,
        };
        self.next = self.next.saturating_add(1);
        let id = self.next;
        self.error = None;
        self.notice = None;
        self.pending = Some(Pending {
            id,
            owner: owner.clone(),
            path: path.clone(),
            open,
        });
        let send = self.send.clone();
        std::thread::spawn(move || {
            let result = fetch(&connection, &owner, &source, &path).map(|_| path);
            if send.send(Reply { id, owner, result }).is_ok() {
                waker();
            }
        });
        Ok(())
    }
    pub fn poll(&mut self, owner: &DraftScope) -> bool {
        let mut changed = false;
        for reply in self.receive.try_iter() {
            let Some(pending) = self
                .pending
                .as_ref()
                .filter(|p| p.id == reply.id && p.owner == reply.owner && &p.owner == owner)
            else {
                continue;
            };
            let open = pending.open;
            let expected = pending.path.clone();
            self.pending = None;
            changed = true;
            match reply.result {
                Ok(path) if path == expected => {
                    if open && self.temp.contains(&path) {
                        match crate::media::local_file_url(&path.to_string_lossy()) {
                            Ok(url) => self.open_url = Some(url),
                            Err(error) => self.error = Some(error),
                        }
                    } else if !open {
                        self.notice = Some("File saved to the selected destination".into());
                    }
                }
                Ok(_) => {
                    self.error = Some("The file transfer returned a different destination".into())
                }
                Err(error) => self.error = Some(error),
            }
        }
        changed
    }
}
fn fetch(
    connection: &Connection,
    owner: &DraftScope,
    source: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    if source.starts_with("data:") {
        let (_, bytes) = crate::chat::media::decode_data_url(source)?;
        return crate::media::write_private(path, &bytes);
    }
    let mut gateway = Gateway::connect(connection)?;
    if source.starts_with("https://") || source.starts_with("http://") {
        let value = gateway.page_request(
            &owner.profile,
            &PageRequest::get("/api/media/proxy")
                .query("url", source)
                .query("session_id", owner.session.as_deref().unwrap_or("")),
        )?;
        let data = value
            .as_str()
            .or(value["data_url"].as_str())
            .or(value["dataUrl"].as_str())
            .or(value["data"].as_str())
            .ok_or("The agent returned no file data")?;
        let (_, bytes) = crate::chat::media::decode_data_url(data)?;
        crate::media::write_private(path, &bytes)
    } else {
        let value = gateway.page_request(
            &owner.profile,
            &PageRequest::get("/api/fs/download")
                .query("path", source)
                .query("session_id", owner.session.as_deref().unwrap_or(""))
                .download_to(path.to_path_buf())
                .timeout_secs(3600),
        )?;
        if value["ok"] == true && value["downloaded"] == true {
            Ok(())
        } else {
            Err("The agent did not confirm the file download".into())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::Arc,
    };
    #[test]
    fn authenticated_child_download_keeps_profile_session_source_and_native_destination() {
        let root =
            std::env::temp_dir().join(format!("hermes-child-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("answer.txt");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ws = tungstenite::accept(stream).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            let first = request.lines().next().unwrap();
            let target = first.split_whitespace().nth(1).unwrap();
            let url = url::Url::parse(&format!("http://localhost{target}")).unwrap();
            let query: HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(url.path(), "/api/fs/download");
            assert_eq!(query.get("profile").unwrap(), "work");
            assert_eq!(query.get("session_id").unwrap(), "child");
            assert_eq!(query.get("path").unwrap(), "/agent/answer.txt");
            assert_eq!(query.get("token").unwrap(), "synthetic");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nchild result"
            )
            .unwrap();
        });
        let owner = DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 9,
            profile: "work".into(),
            session: Some("child".into()),
        };
        let mut files = Files::new();
        files.init(Arc::new(|| {}));
        files
            .start(
                Connection::Url {
                    endpoint: format!("http://{addr}"),
                    token: Some("synthetic".into()),
                },
                owner.clone(),
                "/agent/answer.txt".into(),
                "answer.txt".into(),
                Some(path.clone()),
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while files.busy() && Instant::now() < until {
            files.poll(&owner);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!files.busy());
        assert!(files.error.is_none(), "{:?}", files.error);
        assert_eq!(std::fs::read(&path).unwrap(), b"child result");
        assert!(
            files.open_url.is_none(),
            "Download must never launch the OS"
        );
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn explicit_inline_open_uses_private_native_copy_and_wrong_owner_cannot_publish_it() {
        let owner = DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 1,
            profile: "work".into(),
            session: Some("child".into()),
        };
        let other = DraftScope {
            profile: "other".into(),
            ..owner.clone()
        };
        let mut files = Files::new();
        files.init(Arc::new(|| {}));
        files
            .start(
                Connection::Url {
                    endpoint: "http://127.0.0.1:1".into(),
                    token: None,
                },
                owner.clone(),
                "data:text/plain;base64,Y2hpbGQ=".into(),
                "../child.txt".into(),
                None,
            )
            .unwrap();
        let actual = files.receive.recv_timeout(Duration::from_secs(2)).unwrap();
        // A delayed response from another child cannot open its file in this one.
        files
            .send
            .send(Reply {
                id: files.next,
                owner: other,
                result: Ok(std::path::PathBuf::from("/tmp/foreign")),
            })
            .unwrap();
        files.poll(&owner);
        assert!(files.open_url.is_none());
        let pending = files.pending.as_ref().unwrap();
        let path = pending.path.clone();
        files.send.send(actual).unwrap();
        files.poll(&owner);
        assert!(files.temp.contains(&path));
        assert!(files.open_url.as_ref().unwrap().starts_with("file://"));
        assert_eq!(std::fs::read(path).unwrap(), b"child");
    }
}
