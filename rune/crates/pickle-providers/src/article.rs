//! Public-page download with one validated, pinned DNS resolution and an inert HTML parser.
use futures_util::StreamExt;
use pickle_core::{PickleError, WebReference};
use scraper::{ElementRef, Html, Selector};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    local_budget::truncate_utf8,
    transport::{check_cancel, error},
};

const HTML_BYTES: usize = 2_000_000;
const ARTICLE_BYTES: usize = 12_000;

pub fn public_article_url(raw: &str) -> Result<Url, PickleError> {
    if WebReference::public_url(raw).is_none() {
        return Err(error(
            "This address can’t be used as a public page reference.",
        ));
    }
    if raw.len() > 4096 {
        return Err(error("This page address is too long."));
    }
    let mut url = Url::parse(raw).map_err(|_| error("Enter a public webpage address."))?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if !matches!(url.scheme(), "http" | "https")
        || host.is_empty()
        || !host.contains('.')
        || !url.username().is_empty()
        || url.password().is_some()
        || host.parse::<IpAddr>().is_ok()
        || !host.chars().any(|c| c.is_ascii_alphabetic())
        || [".local", ".localhost", ".internal", ".test", ".invalid"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
        || !matches!(url.port_or_known_default(), Some(80 | 443))
    {
        return Err(error(
            "This address can’t be used as a public page reference.",
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

fn ipv4_public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
        || (a == 198 && ((18..=19).contains(&b) || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113))
}
pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_public(ip),
        IpAddr::V6(ip) => {
            let parts = ip.segments();
            (parts[0] & 0xe000) == 0x2000
                && !(parts[0] == 0x2001 && (parts[1] < 0x0200 || parts[1] == 0x0db8))
                && parts[0] != 0x2002
                && !(parts[0] == 0x3fff && parts[1] < 0x1000)
        }
    }
}

pub async fn fetch_public_reference(
    raw: &str,
    selection: &str,
    strict_offline: bool,
    cancel: CancellationToken,
) -> Result<WebReference, PickleError> {
    if strict_offline {
        return Err(error("Offline mode blocks public webpage fetching."));
    }
    if selection.len() > 12_000 {
        return Err(error("Select a shorter passage for page context."));
    }
    let url = public_article_url(raw)?;
    let host = url.host_str().unwrap().to_owned();
    let port = url.port_or_known_default().unwrap();
    let work = async {
        let addresses = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| error("This webpage address could not be resolved."))?
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
            return Err(error(
                "This address isn’t a public webpage. Use the browser extension for loaded content.",
            ));
        }
        check_cancel(&cancel)?;
        // Override DNS with the already-validated addresses; URL host remains intact for TLS/SNI.
        let client = reqwest::Client::builder()
            .resolve_to_addrs(&host, &addresses)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|_| error("Could not initialize webpage fetching."))?;
        let response = client
            .get(url.clone())
            .header("Accept", "text/html,application/xhtml+xml")
            .send()
            .await
            .map_err(|_| error("This page could not be fetched. Try the browser extension."))?;
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if response.status().as_u16() != 200
            || !matches!(mime.as_str(), "text/html" | "application/xhtml+xml")
        {
            return Err(error(
                "This page needs browser access. Use the Pickle browser extension.",
            ));
        }
        if response
            .content_length()
            .is_some_and(|n| n > HTML_BYTES as u64)
        {
            return Err(error("This page is too large."));
        }
        let mut stream = response.bytes_stream();
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            check_cancel(&cancel)?;
            let chunk = chunk.map_err(|_| error("The webpage download was interrupted."))?;
            if data.len().saturating_add(chunk.len()) > HTML_BYTES {
                return Err(error("This page is too large."));
            }
            data.extend_from_slice(&chunk);
        }
        let html = String::from_utf8(data)
            .map_err(|_| error("Use the browser extension to read this page’s text encoding."))?;
        let selected = selection.to_owned();
        let url = url.to_string();
        tokio::task::spawn_blocking(move || extract_article_html(&html, &url, &selected))
            .await
            .map_err(|_| error("The webpage could not be parsed."))?
    };
    tokio::select! {
        _ = cancel.cancelled() => Err(PickleError::Cancelled),
        result = tokio::time::timeout(Duration::from_secs(5), work) => result.map_err(|_|error("Page context took too long. Use your selection or the browser extension."))?,
    }
}

fn blocked(element: ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .chain(std::iter::once(element))
        .any(|node| {
            matches!(
                node.value().name(),
                "script"
                    | "style"
                    | "noscript"
                    | "template"
                    | "iframe"
                    | "nav"
                    | "footer"
                    | "header"
                    | "input"
                    | "textarea"
                    | "select"
                    | "button"
                    | "form"
                    | "svg"
                    | "canvas"
            ) || node.value().attr("hidden").is_some()
                || node.value().attr("aria-hidden") == Some("true")
                || node
                    .value()
                    .attr("contenteditable")
                    .is_some_and(|v| v != "false")
        })
}

fn plain_text(root: ElementRef<'_>) -> String {
    let mut text = String::new();
    for node in root.descendants() {
        if let scraper::node::Node::Text(value) = node.value() {
            if node.parent().and_then(ElementRef::wrap).is_none_or(blocked) {
                continue;
            }
            for word in value.split_whitespace() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(word);
                if text.len() >= HTML_BYTES {
                    return text;
                }
            }
        }
    }
    text
}

/// Pure HTML parsing: no JavaScript, navigation, subresources, cookies, or hidden browser.
/// This is a conservative portable extractor, not an implementation of Mozilla Readability.
pub fn extract_article_html(
    html: &str,
    raw_url: &str,
    selection: &str,
) -> Result<WebReference, PickleError> {
    let url = public_article_url(raw_url)?;
    if html.len() > HTML_BYTES {
        return Err(error("This page is too large."));
    }
    let document = Html::parse_document(html);
    if document.tree.nodes().count() > 20_000 {
        return Err(error(
            "This page is too complex. Use the browser extension.",
        ));
    }
    let select = |value: &str| Selector::parse(value).expect("static selector");
    let root = document
        .select(&select("article, main, [role=main]"))
        .filter(|node| !blocked(*node))
        .take(32)
        .max_by_key(|node| plain_text(*node).len())
        .or_else(|| document.select(&select("body")).next())
        .ok_or_else(|| error("No article text was found on this page."))?;
    let title = document
        .select(&select("title"))
        .next()
        .map(plain_text)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| url.host_str().unwrap().into());
    let mut paragraphs = root
        .select(&select("p, h1, h2, h3, h4, li, blockquote, pre"))
        .filter(|node| !blocked(*node))
        .filter(|node| {
            !node
                .ancestors()
                .take_while(|ancestor| ancestor.id() != root.id())
                .filter_map(ElementRef::wrap)
                .any(|ancestor| {
                    matches!(ancestor.value().name(), "p" | "li" | "blockquote" | "pre")
                })
        })
        .map(plain_text)
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        paragraphs.push(plain_text(root));
    }
    let text = excerpt(&paragraphs, selection, ARTICLE_BYTES);
    let reference = WebReference {
        url: url.to_string(),
        title: truncate_utf8(&title, 600).into(),
        kind: "article".into(),
        text,
        timestamp: None,
    };
    reference.validate()?;
    Ok(reference)
}

fn excerpt(paragraphs: &[String], selection: &str, budget: usize) -> String {
    let words = selection
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 3)
        .map(str::to_lowercase)
        .collect::<std::collections::BTreeSet<_>>();
    let mut ranked = paragraphs
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                i,
                words
                    .iter()
                    .filter(|word| p.to_lowercase().contains(word.as_str()))
                    .count(),
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(i, score)| (std::cmp::Reverse(*score), *i));
    let mut chosen = std::collections::BTreeSet::new();
    let mut bytes = 0;
    for (index, _) in &ranked {
        for candidate in [
            Some(*index),
            index.checked_sub(1),
            index.checked_add(1).filter(|i| *i < paragraphs.len()),
        ]
        .into_iter()
        .flatten()
        {
            if !chosen.contains(&candidate) && bytes + paragraphs[candidate].len() + 2 <= budget {
                chosen.insert(candidate);
                bytes += paragraphs[candidate].len() + 2;
            }
        }
    }
    if chosen.is_empty() {
        return ranked
            .first()
            .map(|(i, _)| truncate_utf8(&paragraphs[*i], budget).to_owned())
            .unwrap_or_default();
    }
    chosen
        .into_iter()
        .map(|i| paragraphs[i].clone())
        .collect::<Vec<_>>()
        .join("\n\n")
}
