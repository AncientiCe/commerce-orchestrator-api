//! Optional Web Bot Auth identity verification and bounded HTTPS key discovery.
use crate::state::AppState;
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use orchestrator_api::{MessageComponents, WebBotIdentity, WebBotSignature};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub struct DirectoryDocument {
    pub jwks: Value,
    pub max_age_seconds: u64,
}
/// Implementations must authenticate the directory URL and never substitute keys
/// fetched from another URL. Production uses HTTPS with public-address pinning.
#[axum::async_trait]
pub trait DirectoryResolver: Send + Sync {
    async fn resolve(&self, url: &str) -> Result<DirectoryDocument, String>;
}

pub struct HttpsDirectories {
    allowed: BTreeSet<String>,
}
impl HttpsDirectories {
    pub fn new(urls: Vec<String>) -> Result<Self, String> {
        if urls.is_empty() || urls.len() > 64 {
            return Err("configure between 1 and 64 directory URLs".into());
        }
        let mut allowed = BTreeSet::new();
        for value in urls {
            let url = url::Url::parse(&value).map_err(|_| "invalid directory URL")?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || url.query().is_some()
            {
                return Err(
                    "directory URLs require HTTPS without credentials, query, or fragment".into(),
                );
            }
            allowed.insert(url.to_string());
        }
        Ok(Self { allowed })
    }
}
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19)))
        }
        IpAddr::V6(ip) => {
            // Accept only global unicast; reject transition/mapped and special ranges.
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && segments[1] <= 0x1ff)
                && !(segments[0] == 0x2001 && segments[1] == 0xdb8)
                && segments[0] != 0x2002
        }
    }
}
#[axum::async_trait]
impl DirectoryResolver for HttpsDirectories {
    async fn resolve(&self, value: &str) -> Result<DirectoryDocument, String> {
        if !self.allowed.contains(value) {
            return Err("directory URL is not allowed".into());
        }
        let url = url::Url::parse(value).map_err(|_| "invalid directory URL")?;
        let host = url.host_str().ok_or("missing directory host")?;
        let port = url
            .port_or_known_default()
            .ok_or("missing directory port")?;
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| "directory DNS timeout")?
        .map_err(|_| "directory DNS failure")?
        .collect();
        if addresses.is_empty() || addresses.iter().any(|a| !public_address(a.ip())) {
            return Err("directory resolved to a prohibited address".into());
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| "directory client failure")?;
        let mut response=client.get(url.clone()).header("accept","application/http-message-signatures-directory+json, application/jwk-set+json, application/json").send().await.map_err(|_|"directory fetch failed")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("directory did not return 200".into());
        }
        if url.path() == "/.well-known/http-message-signatures-directory"
            && response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(';').next().unwrap_or("").trim())
                != Some("application/http-message-signatures-directory+json")
        {
            return Err("incorrect directory media type".into());
        }
        let cache_control = response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let max_age = if cache_control
            .split(',')
            .any(|v| matches!(v.trim(), "no-store" | "no-cache"))
        {
            0
        } else {
            cache_control
                .split(',')
                .find_map(|v| {
                    v.trim()
                        .strip_prefix("max-age=")?
                        .trim_matches('"')
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(60)
                .min(300)
        };
        if response.content_length().is_some_and(|size| size > 65536) {
            return Err("directory too large".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "directory read failed")?
        {
            if bytes.len() + chunk.len() > 65536 {
                return Err("directory too large".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let jwks = serde_json::from_slice(&bytes).map_err(|_| "invalid directory JSON")?;
        Ok(DirectoryDocument {
            jwks,
            max_age_seconds: max_age,
        })
    }
}

pub struct WebBotAuth {
    resolver: Arc<dyn DirectoryResolver>,
    cache: Mutex<BTreeMap<String, (Instant, Value)>>,
}
impl WebBotAuth {
    pub fn new(resolver: Arc<dyn DirectoryResolver>) -> Self {
        Self {
            resolver,
            cache: Mutex::new(BTreeMap::new()),
        }
    }
    async fn keys(&self, url: &str) -> Result<Value, String> {
        let mut cache = self.cache.lock().await;
        cache.retain(|_, (until, _)| *until > Instant::now());
        if let Some((_, keys)) = cache.get(url) {
            return Ok(keys.clone());
        }
        let started = Instant::now();
        let document = self.resolver.resolve(url).await;
        orchestrator_observability::observe_operation(
            "web_bot_auth_discovery",
            if document.is_ok() { "success" } else { "error" },
            started.elapsed().as_secs_f64(),
        );
        let document = document?;
        if document
            .jwks
            .get("keys")
            .and_then(Value::as_array)
            .is_none_or(|keys| keys.len() > 128)
        {
            return Err("invalid or excessive directory keys".into());
        }
        if document.max_age_seconds > 0 {
            if cache.len() >= 64 {
                cache.clear();
            }
            cache.insert(
                url.into(),
                (
                    Instant::now() + Duration::from_secs(document.max_age_seconds.min(300)),
                    document.jwks.clone(),
                ),
            );
        }
        Ok(document.jwks)
    }
    async fn verify(
        &self,
        components: &MessageComponents,
        input: &str,
        signature: &str,
        agent: &str,
    ) -> Result<WebBotIdentity, String> {
        let proofs =
            WebBotSignature::parse(input, agent).map_err(|_| "invalid Web Bot Auth signature")?;
        if proofs.len() > 8 {
            return Err("too many signatures".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "clock error")?
            .as_secs() as i64;
        for proof in proofs {
            if let Ok(keys) = self.keys(&proof.directory_url).await {
                if let Ok(identity) = proof.verify(components, signature, &keys, now) {
                    return Ok(identity);
                }
            }
        }
        Err("Web Bot Auth verification failed".into())
    }
}
fn rejected() -> Response {
    orchestrator_observability::incr("web_bot_auth_rejected_total");
    (StatusCode::FORBIDDEN,[("accept-signature","sig1=(\"@authority\" \"signature-agent\";key=\"sig1\");tag=\"web-bot-auth\";created;expires;keyid")],axum::Json(serde_json::json!({"code":"web_bot_auth_required","message":"A valid Web Bot Auth signature is required"}))).into_response()
}
pub async fn verify_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(auth) = &state.web_bot_auth else {
        return next.run(request).await;
    };
    if !request.uri().path().starts_with("/api/v1/") {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let (Some(input), Some(signature), Some(agent)) = (
        header("signature-input"),
        header("signature"),
        header("signature-agent"),
    ) else {
        return rejected();
    };
    let bytes = match to_bytes(body, 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let mut components = MessageComponents::new()
        .with("@method", parts.method.as_str())
        .with("@path", parts.uri.path());
    let authority = header("host")
        .or_else(|| parts.uri.authority().map(ToString::to_string))
        .unwrap_or_default()
        .to_ascii_lowercase();
    components.insert("@authority", &authority);
    if let Some(query) = parts.uri.query() {
        components.insert("@query", &format!("?{query}"));
    }
    let scheme = url::Url::parse(&state.discovery_base_url)
        .ok()
        .map(|u| u.scheme().to_string())
        .unwrap_or_else(|| "https".into());
    components.insert(
        "@target-uri",
        &format!(
            "{scheme}://{authority}{}",
            parts
                .uri
                .path_and_query()
                .map(|v| v.as_str())
                .unwrap_or("/")
        ),
    );
    for name in parts.headers.keys() {
        let values = parts
            .headers
            .get_all(name)
            .iter()
            .map(|v| v.to_str().map(str::trim))
            .collect::<Result<Vec<_>, _>>();
        let Ok(values) = values else {
            return rejected();
        };
        components.insert(name.as_str(), &values.join(", "));
    }
    if let Some(digest) = header("content-digest") {
        if orchestrator_api::verify_content_digest(&digest, &bytes).is_err() {
            return rejected();
        }
    }
    match auth.verify(&components, &input, &signature, &agent).await {
        Ok(identity) => {
            orchestrator_observability::incr("web_bot_auth_verified_total");
            parts.extensions.insert(identity);
            next.run(Request::from_parts(parts, Body::from(bytes)))
                .await
        }
        Err(_) => rejected(),
    }
}
