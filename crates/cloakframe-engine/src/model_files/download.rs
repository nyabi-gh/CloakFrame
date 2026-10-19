//! Downloading a model over HTTPS, only from the hosts its pinned URL and GitHub's redirects use
//! (plan 3, 4.6). The bytes go to a temporary file in the model folder, hashed as they arrive, and
//! the file gets the model's name only if they are the pinned file's.

use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use cloakframe_core::detection::ModelId;
use ureq::Agent;
use ureq::http::{Uri, header};
use ureq::tls::{RootCerts, TlsConfig};

use super::{InstallError, ModelStore, model_file};
use crate::cancel::CancelToken;

/// GitHub, where every pinned URL points, and the hosts it hands downloads to: release assets
/// (served from `objects.` before `release-assets.githubusercontent.com`) and Git LFS files, which
/// YuNet's is.
const HOSTS: [&str; 4] = [
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
    "media.githubusercontent.com",
];
const MAX_REDIRECTS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(30);
/// 1.12.1's limit for a whole download.
const BODY_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The HTTP boundary (plan 4.1), so tests can stand in for the network.
pub trait Fetch {
    /// The body of `url`, after the redirects the allowlist lets through.
    fn get(&self, url: &str) -> Result<Body, FetchError>;
}

pub struct Body {
    pub length: Option<u64>,
    pub reader: Box<dyn Read + Send>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// A URL that is not HTTPS to an allowed host.
    NotAllowed(String),
    Status(u16),
    TooManyRedirects,
    Network(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAllowed(url) => write!(f, "{url} is not an allowed download address"),
            Self::Status(code) => write!(f, "the server answered HTTP {code}"),
            Self::TooManyRedirects => f.write_str("too many redirects"),
            Self::Network(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for FetchError {}

#[derive(Debug)]
pub enum DownloadError {
    Fetch(FetchError),
    Install(InstallError),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(e) => e.fmt(f),
            Self::Install(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for DownloadError {}

/// Downloads `model` into `store` from its pinned URL. `progress` gets the bytes received so
/// far; the total is the pinned size.
pub fn download(
    store: &ModelStore,
    model: ModelId,
    fetch: &dyn Fetch,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    let pinned = model_file(model);
    let mut body = fetch.get(pinned.url).map_err(DownloadError::Fetch)?;
    if body.length.is_some_and(|n| n > pinned.size) {
        return Err(DownloadError::Install(InstallError::TooLarge));
    }
    store
        .install(model, &mut body.reader, cancel, progress)
        .map_err(DownloadError::Install)
}

#[derive(Debug, Clone, Copy)]
struct Allowlist {
    scheme: &'static str,
    hosts: &'static [&'static str],
    port: Option<u16>,
}

impl Allowlist {
    const DOWNLOADS: Self = Self {
        scheme: "https",
        hosts: &HOSTS,
        port: None,
    };

    fn check(&self, url: &str) -> Result<Uri, FetchError> {
        let refused = || FetchError::NotAllowed(url.to_owned());
        let uri: Uri = url.parse().map_err(|_| refused())?;
        let authority = uri.authority().ok_or_else(refused)?;
        let host = authority.host().to_ascii_lowercase();
        let allowed = uri.scheme_str() == Some(self.scheme)
            && !authority.as_str().contains('@')
            && authority.port_u16() == self.port
            && self.hosts.contains(&host.as_str());
        if allowed { Ok(uri) } else { Err(refused()) }
    }
}

/// Where a redirect from `from` points: an absolute URL, or a path on the same host.
fn redirect_target(from: &Uri, location: &str) -> Option<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Some(location.to_owned());
    }
    if location.starts_with('/') && !location.starts_with("//") {
        let scheme = from.scheme_str()?;
        let authority = from.authority()?;
        return Some(format!("{scheme}://{authority}{location}"));
    }
    None
}

/// Downloads through the system's certificate trust (rustls-platform-verifier), following each
/// redirect only after checking it against the allowlist. Proxies come from the usual
/// environment variables (`HTTPS_PROXY`, `NO_PROXY`); TLS still ends at the allowed host.
pub struct Https {
    agent: Agent,
    allowlist: Allowlist,
}

impl Default for Https {
    fn default() -> Self {
        Self::with(Allowlist::DOWNLOADS)
    }
}

impl Https {
    fn with(allowlist: Allowlist) -> Self {
        let tls = TlsConfig::builder()
            .root_certs(RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = Agent::config_builder()
            .https_only(allowlist.scheme == "https")
            .max_redirects(0)
            .http_status_as_error(false)
            .tls_config(tls)
            .timeout_connect(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .build()
            .new_agent();
        Self { agent, allowlist }
    }
}

impl Fetch for Https {
    fn get(&self, url: &str) -> Result<Body, FetchError> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let uri = self.allowlist.check(&url)?;
            let response = self
                .agent
                .get(uri.clone())
                .call()
                .map_err(|e| FetchError::Network(e.to_string()))?;
            let status = response.status();
            if status.is_redirection() {
                url = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|l| l.to_str().ok())
                    .and_then(|l| redirect_target(&uri, l))
                    .ok_or(FetchError::Status(status.as_u16()))?;
                continue;
            }
            if status != 200 {
                return Err(FetchError::Status(status.as_u16()));
            }
            let length = response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|l| l.to_str().ok())
                .and_then(|l| l.parse().ok());
            return Ok(Body {
                length,
                reader: Box::new(response.into_body().into_reader()),
            });
        }
        Err(FetchError::TooManyRedirects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_files::MODEL_FILES;
    use crate::test_support::TestDir;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn only_https_to_the_allowed_hosts_passes() {
        let allow = Allowlist::DOWNLOADS;
        for file in &MODEL_FILES {
            assert!(allow.check(file.url).is_ok(), "{}", file.url);
        }
        for url in [
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/x?sp=r",
            "https://media.githubusercontent.com/media/opencv/opencv_zoo/x/model.onnx",
            "https://objects.githubusercontent.com/x",
            "https://GitHub.com/x",
        ] {
            assert!(allow.check(url).is_ok(), "{url}");
        }
        for url in [
            "http://github.com/x",
            "https://github.com.example/x",
            "https://example.com/github.com/x",
            "https://raw.githubusercontent.com/x",
            "https://user@github.com/x",
            "https://github.com:8443/x",
            "ftp://github.com/x",
            "github.com/x",
            "https://140.82.121.4/x",
        ] {
            assert_eq!(
                allow.check(url).unwrap_err(),
                FetchError::NotAllowed(url.to_owned()),
                "{url}"
            );
        }
    }

    #[test]
    fn redirects_resolve_against_the_page_they_come_from() {
        let from: Uri = "https://github.com/a/b".parse().unwrap();
        assert_eq!(
            redirect_target(&from, "https://media.githubusercontent.com/m").as_deref(),
            Some("https://media.githubusercontent.com/m")
        );
        assert_eq!(
            redirect_target(&from, "/c/d?e").as_deref(),
            Some("https://github.com/c/d?e")
        );
        assert_eq!(redirect_target(&from, "//evil.example/x"), None);
        assert_eq!(redirect_target(&from, "relative"), None);
    }

    /// Answers each connection with the next canned response; returns the port and the paths
    /// requested.
    fn serve(responses: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let mut paths = Vec::new();
            for response in responses {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                paths.push(line.split(' ').nth(1).unwrap().to_owned());
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header == "\r\n" || header.is_empty() {
                        break;
                    }
                }
                let mut stream = reader.into_inner();
                stream.write_all(&response).unwrap();
            }
            paths
        });
        (port, handle)
    }

    fn redirect(to: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    const LOCAL: [&str; 1] = ["127.0.0.1"];

    fn local(port: u16) -> Https {
        Https::with(Allowlist {
            scheme: "http",
            hosts: &LOCAL,
            port: Some(port),
        })
    }

    #[test]
    fn allowed_redirects_are_followed_to_the_body() {
        let (port, server) = serve(vec![
            redirect("/assets/model?sig=1"),
            redirect("/final"),
            ok(b"model bytes"),
        ]);
        let mut body = local(port)
            .get(&format!("http://127.0.0.1:{port}/start"))
            .unwrap();
        assert_eq!(body.length, Some(11));
        let mut bytes = Vec::new();
        body.reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"model bytes");
        assert_eq!(
            server.join().unwrap(),
            ["/start", "/assets/model?sig=1", "/final"]
        );
    }

    #[test]
    fn a_redirect_off_the_allowlist_is_not_followed() {
        let (port, server) = serve(vec![redirect("http://localhost:1/elsewhere")]);
        assert_eq!(
            local(port)
                .get(&format!("http://127.0.0.1:{port}/start"))
                .err(),
            Some(FetchError::NotAllowed(
                "http://localhost:1/elsewhere".into()
            ))
        );
        assert_eq!(server.join().unwrap(), ["/start"]);
    }

    #[test]
    fn errors_and_endless_redirects_stop_the_download() {
        let missing = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (port, server) = serve(vec![missing.to_vec()]);
        assert_eq!(
            local(port).get(&format!("http://127.0.0.1:{port}/x")).err(),
            Some(FetchError::Status(404))
        );
        server.join().unwrap();

        let loops = (0..=MAX_REDIRECTS).map(|_| redirect("/again")).collect();
        let (port, server) = serve(loops);
        assert_eq!(
            local(port).get(&format!("http://127.0.0.1:{port}/x")).err(),
            Some(FetchError::TooManyRedirects)
        );
        assert_eq!(server.join().unwrap().len(), MAX_REDIRECTS + 1);
    }

    /// Serves fixed bytes for every URL.
    struct Serves(Vec<u8>, Option<u64>);

    impl Fetch for Serves {
        fn get(&self, _: &str) -> Result<Body, FetchError> {
            Ok(Body {
                length: self.1,
                reader: Box::new(std::io::Cursor::new(self.0.clone())),
            })
        }
    }

    #[test]
    fn a_body_announced_as_too_large_is_not_read() {
        let dir = TestDir::new();
        let store = ModelStore::new(dir.path().join("models"));
        let size = MODEL_FILES[1].size;
        let result = download(
            &store,
            ModelId::YuNet,
            &Serves(Vec::new(), Some(size + 1)),
            &CancelToken::new(),
            &mut |_| panic!("nothing is read"),
        );
        assert!(matches!(
            result,
            Err(DownloadError::Install(InstallError::TooLarge))
        ));
        let result = download(
            &store,
            ModelId::YuNet,
            &Serves(vec![0; 100], Some(100)),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert!(matches!(
            result,
            Err(DownloadError::Install(InstallError::Truncated {
                received: 100
            }))
        ));
        assert!(!store.path(ModelId::YuNet).exists());
    }
}
