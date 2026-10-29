use super::*;
pub(super) struct Stager {
    send: Sender<Result<Vec<String>, String>>,
    receive: Receiver<Result<Vec<String>, String>>,
}
impl Stager {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        Self { send, receive }
    }
    pub fn start(
        &self,
        connection: Connection,
        profile: String,
        paths: Vec<PathBuf>,
        cancel: Arc<AtomicBool>,
    ) {
        let send = self.send.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut output = vec![];
                for path in paths {
                    if cancel.load(Ordering::Acquire) {
                        return Err("The terminal file drop was cancelled".into());
                    }
                    output.push(crate::gateway::terminal::stage(
                        &connection,
                        &profile,
                        &path,
                    )?);
                }
                Ok(output)
            })();
            if !cancel.load(Ordering::Acquire) {
                let _ = send.send(result);
            }
        });
    }
    pub fn poll(&self) -> Vec<Result<Vec<String>, String>> {
        self.receive.try_iter().collect()
    }
}
