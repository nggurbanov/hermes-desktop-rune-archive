//! External media has no gateway object, authentication material, cookies or proxy state.
use super::{Cancel, Metadata, Method, Request};
use std::{io::Write, time::Duration};

pub(super) struct Http {
    url: url::Url,
    client: reqwest::Client,
    runtime: tokio::runtime::Runtime,
}
impl Http {
    pub fn new(value: &str) -> Result<Self, String> {
        let url = url::Url::parse(value).map_err(|_| "Invalid external media URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "External media requires an HTTP(S) URL without embedded credentials".into(),
            );
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "The external media transport could not start")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "The external media runtime could not start")?;
        Ok(Self {
            url,
            client,
            runtime,
        })
    }
    pub fn transfer(
        &mut self,
        request: &Request,
        cancel: &Cancel,
        mut validate: impl FnMut(&Metadata) -> Result<(), String>,
        output: &mut impl Write,
    ) -> Result<Metadata, String> {
        if cancel.cancelled() {
            return Err("External media transfer cancelled".into());
        }
        let method = if request.method == Method::Head {
            reqwest::Method::HEAD
        } else {
            reqwest::Method::GET
        };
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity"),
        );
        for (name, value) in [("range", &request.range), ("if-range", &request.if_range)] {
            if let Some(value) = value {
                headers.insert(
                    reqwest::header::HeaderName::from_static(name),
                    value
                        .parse()
                        .map_err(|_| "Invalid external media range header")?,
                );
            }
        }
        self.runtime.block_on(async {
            let send = self.client.request(method,self.url.clone()).headers(headers).send();
            let mut response = tokio::select! {
                biased;
                _ = cancel.wait() => return Err("External media transfer cancelled".into()),
                result = send => result.map_err(|_| "External media transfer failed (network, TLS or timeout)")?,
            };
            let header = |name:&str| response.headers().get(name).and_then(|value|value.to_str().ok()).map(str::to_owned);
            let mut metadata = Metadata {
                status:response.status().as_u16(), content_type:header("content-type"),
                content_length:header("content-length").and_then(|value|value.parse().ok()),
                content_range:header("content-range"),accept_ranges:header("accept-ranges"),
                etag:header("etag"),last_modified:header("last-modified"),received:0,
            };
            validate(&metadata)?;
            if request.method == Method::Head || !response.status().is_success() { return Ok(metadata); }
            if metadata.content_length.is_some_and(|length|length > request.limit) {
                return Err("The external media response exceeds its range limit".into());
            }
            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel.wait() => return Err("External media transfer cancelled".into()),
                    result = response.chunk() => result.map_err(|_| "The external media transfer was interrupted")?,
                };
                let Some(chunk) = chunk else { break; };
                metadata.received = metadata.received.checked_add(chunk.len() as u64)
                    .filter(|length|*length <= request.limit).ok_or("The external media response exceeds its range limit")?;
                output.write_all(&chunk).map_err(|_| "The external media response could not be buffered")?;
            }
            Ok(metadata)
        })
    }
}
