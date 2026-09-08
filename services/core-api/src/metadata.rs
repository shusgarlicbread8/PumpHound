use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use tokio::net::lookup_host;

const MAX_BYTES: usize = 256 * 1024;
const MAX_REDIRECTS: u32 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SanitizedMetadata {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub description: Option<String>,
    pub image_cached: bool,
    pub blocked: bool,
    pub reason: Option<String>,
    pub payload_sha256: Option<String>,
}

pub async fn fetch_metadata(uri: &str) -> SanitizedMetadata {
    match fetch_inner(uri, 0).await {
        Ok(m) => m,
        Err(e) => SanitizedMetadata {
            name: None,
            symbol: None,
            description: None,
            image_cached: false,
            blocked: true,
            reason: Some(e.to_string()),
            payload_sha256: None,
        },
    }
}

async fn fetch_inner(uri: &str, depth: u32) -> anyhow::Result<SanitizedMetadata> {
    if depth > MAX_REDIRECTS {
        anyhow::bail!("too many redirects");
    }
    let url = reqwest::Url::parse(uri).context("url")?;
    if url.scheme() != "https" && url.scheme() != "http" {
        anyhow::bail!("scheme not allowed");
    }
    let host = url.host_str().context("host")?;
    deny_private_host(host).await?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let res = client.get(url.clone()).send().await?;
    if res.status().is_redirection() {
        let loc = res
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .context("redirect without location")?;
        return Box::pin(fetch_inner(loc, depth + 1)).await;
    }
    let bytes = res.bytes().await?;
    if bytes.len() > MAX_BYTES {
        anyhow::bail!("response too large");
    }
    let hash = pumphound_domain::payload_sha256(&bytes);
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({}));
    Ok(SanitizedMetadata {
        name: v.get("name").and_then(|x| x.as_str()).map(sanitize_text),
        symbol: v.get("symbol").and_then(|x| x.as_str()).map(sanitize_text),
        description: v
            .get("description")
            .and_then(|x| x.as_str())
            .map(sanitize_text),
        image_cached: false,
        blocked: false,
        reason: None,
        payload_sha256: Some(hash),
    })
}

fn sanitize_text(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>()
        .replace(['<', '>'], "")
}

async fn deny_private_host(host: &str) -> anyhow::Result<()> {
    if host == "localhost" || host.ends_with(".localhost") {
        anyhow::bail!("private host blocked");
    }
    let addrs: Vec<SocketAddr> = lookup_host((host, 443)).await?.collect();
    if addrs.is_empty() {
        anyhow::bail!("dns empty");
    }
    for a in addrs {
        if is_blocked_ip(a.ip()) {
            anyhow::bail!("private or metadata IP blocked");
        }
    }
    Ok(())
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_multicast()
                || v.octets()[0] == 169 && v.octets()[1] == 254
                || v.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v) => v.is_loopback() || v.is_multicast() || v.is_unique_local(),
    }
}
