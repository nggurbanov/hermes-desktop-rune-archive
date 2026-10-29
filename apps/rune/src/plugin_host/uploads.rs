//! Back-pressured chunks preserve the original upload limit without huge IPC frames.
use base64::Engine;
use serde_json::{Value, json};
use std::collections::BTreeMap;
const LIMIT: usize = 256 * 1024 * 1024;
const CHUNK: usize = 8 * 1024 * 1024;
pub(super) struct Transferred {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}
struct Receiving {
    request: Value,
    filename: String,
    content_type: String,
    total: usize,
    next: u64,
    bytes: Vec<u8>,
}
#[derive(Default)]
pub(super) struct Uploads {
    receiving: BTreeMap<String, Receiving>,
}
impl Uploads {
    pub fn is_busy(&self) -> bool {
        !self.receiving.is_empty()
    }
    pub fn start(&mut self, payload: &Value) -> Result<Value, String> {
        let id = payload["transfer_id"]
            .as_str()
            .ok_or("The upload has no transfer receipt")?;
        if uuid::Uuid::parse_str(id).is_err() {
            return Err("The upload receipt is invalid".into());
        }
        if !self.receiving.is_empty() {
            return Err("Finish the current plugin upload before starting another".into());
        }
        let total = payload["total_bytes"]
            .as_u64()
            .filter(|n| *n <= LIMIT as u64)
            .ok_or("The plugin upload exceeds the 256 MiB limit")? as usize;
        let filename = payload["filename"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.contains(['\r', '\n']))
            .ok_or("The plugin upload filename is invalid")?
            .to_owned();
        let content_type = payload["content_type"]
            .as_str()
            .unwrap_or("application/octet-stream");
        if content_type.len() > 256 || content_type.contains(['\r', '\n']) {
            return Err("The plugin upload content type is invalid".into());
        }
        self.receiving.insert(
            id.into(),
            Receiving {
                request: payload["request"].clone(),
                filename,
                content_type: content_type.into(),
                total,
                next: 0,
                bytes: Vec::new(),
            },
        );
        Ok(json!({"accepted":true,"chunk_bytes":CHUNK}))
    }
    pub fn chunk(&mut self, payload: &Value) -> Result<Value, String> {
        let id = payload["transfer_id"]
            .as_str()
            .ok_or("The upload has no transfer receipt")?;
        let transfer = self
            .receiving
            .get_mut(id)
            .ok_or("The plugin upload receipt is no longer active")?;
        if payload["index"].as_u64() != Some(transfer.next) {
            return Err("The plugin upload chunk is out of order".into());
        }
        let text = payload["bytes_base64"]
            .as_str()
            .filter(|s| s.len() <= CHUNK.div_ceil(3) * 4)
            .ok_or("The plugin upload chunk exceeds the size limit")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(|_| "The plugin upload chunk encoding is invalid")?;
        if bytes.len() > CHUNK || transfer.bytes.len() + bytes.len() > transfer.total {
            return Err("The plugin upload chunk exceeds its declared size".into());
        }
        transfer.bytes.extend_from_slice(&bytes);
        transfer.next += 1;
        Ok(json!({"received_bytes":transfer.bytes.len()}))
    }
    pub fn abort(&mut self, id: &str) -> Value {
        self.receiving.remove(id);
        json!({"ok":true})
    }
    pub fn take(&mut self, wire: &Value, request: &Value) -> Result<Transferred, String> {
        if let Some(id) = wire["transfer_id"].as_str() {
            let transfer = self
                .receiving
                .remove(id)
                .ok_or("The plugin upload receipt is no longer active")?;
            if transfer.request != *request {
                return Err("The plugin upload request changed while reading its file".into());
            }
            if transfer.bytes.len() != transfer.total {
                return Err("The plugin upload is incomplete; select the file again".into());
            }
            return Ok(Transferred {
                filename: transfer.filename,
                content_type: transfer.content_type,
                bytes: transfer.bytes,
            });
        }
        let encoded = wire["bytes_base64"]
            .as_str()
            .filter(|s| s.len() <= LIMIT.div_ceil(3) * 4)
            .ok_or("The plugin upload has no prepared bytes")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "The plugin upload encoding is invalid")?;
        if bytes.len() > LIMIT {
            return Err("The plugin upload exceeds the 256 MiB limit".into());
        }
        Ok(Transferred {
            filename: wire["filename"].as_str().unwrap_or("upload").into(),
            content_type: wire["contentType"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .into(),
            bytes,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_receipts_preserve_binary_bytes_and_fence_the_final_owner() {
        let mut uploads = Uploads::default();
        let id = uuid::Uuid::new_v4().to_string();
        let request =
            json!({"path":"/api/plugins/proof/upload","profile":"research","method":"POST"});
        uploads
            .start(
                &json!({"transfer_id":id,"total_bytes":3,"filename":"proof.bin","request":request}),
            )
            .unwrap();
        assert!(
            uploads
                .chunk(&json!({"transfer_id":id,"index":1,"bytes_base64":"AP+A"}))
                .is_err()
        );
        uploads
            .chunk(&json!({"transfer_id":id,"index":0,"bytes_base64":"AP+A"}))
            .unwrap();
        assert!(
            uploads
                .take(
                    &json!({"transfer_id":id}),
                    &json!({"path":"/api/plugins/proof/upload","profile":"default","method":"POST"})
                )
                .is_err()
        );
        uploads
            .start(
                &json!({"transfer_id":id,"total_bytes":3,"filename":"proof.bin","request":request}),
            )
            .unwrap();
        uploads
            .chunk(&json!({"transfer_id":id,"index":0,"bytes_base64":"AP+A"}))
            .unwrap();
        let file = uploads.take(&json!({"transfer_id":id}), &request).unwrap();
        assert_eq!(file.bytes, vec![0, 255, 128]);
        assert_eq!(file.filename, "proof.bin");
        assert!(uploads.take(&json!({"transfer_id":id}), &request).is_err());
    }
    #[test]
    fn incomplete_and_oversized_transfers_never_become_a_multipart_request() {
        let mut uploads = Uploads::default();
        let id = uuid::Uuid::new_v4().to_string();
        assert!(
            uploads
                .start(&json!({"transfer_id":id,"total_bytes":LIMIT+1,"filename":"proof.bin"}))
                .is_err()
        );
        uploads
            .start(&json!({"transfer_id":id,"total_bytes":3,"filename":"proof.bin","request":{}}))
            .unwrap();
        assert!(
            uploads
                .take(&json!({"transfer_id":id}), &json!({}))
                .is_err()
        );
    }
}
