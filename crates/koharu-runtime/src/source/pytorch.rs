use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use crate::downloads::Transfer;

/// Filename -> advertised SHA-256 for one simple index URL.
type IndexHashes = HashMap<String, Option<String>>;

static INDEXES: OnceLock<Mutex<HashMap<String, IndexHashes>>> = OnceLock::new();

/// Resolves the SHA-256 advertised by a PEP 503 simple index for a wheel
/// filename, e.g. `https://download.pytorch.org/whl/cu130/torch/`.
///
/// PyTorch's index embeds the digest in each link as a `#sha256=` fragment.
/// Returns `None` when the index omits hashes, the wheel is not listed, or the
/// index is unreachable (callers then fall back to size-only verification).
pub(crate) async fn index_sha256(index: &str, filename: &str) -> Option<String> {
    let cache = INDEXES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hashes) = cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(index).cloned())
        .and_then(|hashes| hashes.get(filename).cloned())
    {
        return hashes;
    }

    let hashes = fetch(index).await?;
    let hashed = hashes.get(filename).and_then(Clone::clone);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(index.to_owned(), hashes);
    }
    hashed
}

async fn fetch(index: &str) -> Option<IndexHashes> {
    let body = Transfer::new()
        .ok()?
        .get(index)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    Some(parse(&body))
}

/// Extracts every wheel link and its `#sha256=` fragment from a simple index.
fn parse(body: &str) -> IndexHashes {
    let mut hashes = HashMap::new();
    for fragment in body.split("href=\"").skip(1) {
        let Some(href) = fragment.split('"').next() else {
            continue;
        };
        let (path, anchor) = href.split_once('#').unwrap_or((href, ""));
        let Some(name) = path
            .rsplit('/')
            .next()
            .filter(|name| name.ends_with(".whl"))
        else {
            continue;
        };
        let sha256 = anchor
            .split("sha256=")
            .nth(1)
            .map(|value| value.split('&').next().unwrap_or(value).to_owned());
        hashes.insert(name.to_owned(), sha256);
    }
    hashes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_index_links() {
        let body = r#"<html><body>
            <a href="torch-2.12.1%2Bcu130-cp312-cp312-win_amd64.whl#sha256=52c5da6a0898d5d3473c02bd304b7a3bc0b72e351c6f3bfa0783e45ef9f4cd61">torch</a>
            <a href="torch-2.12.1%2Bcu130-cp312-cp312-linux_x86_64.whl">torch-linux</a>
        </body></html>"#;
        let parsed = parse(body);
        assert_eq!(
            parsed
                .get("torch-2.12.1%2Bcu130-cp312-cp312-win_amd64.whl")
                .and_then(Clone::clone)
                .as_deref(),
            Some("52c5da6a0898d5d3473c02bd304b7a3bc0b72e351c6f3bfa0783e45ef9f4cd61")
        );
        assert_eq!(
            parsed
                .get("torch-2.12.1%2Bcu130-cp312-cp312-linux_x86_64.whl")
                .and_then(Clone::clone),
            None
        );
        assert!(!parsed.contains_key("index.html"));
    }

    #[test]
    fn simple_index_without_hashes_yields_none() {
        let body = r#"<html><body>
            <a href="torch-2.12.1-cp312-cp312-win_amd64.whl">torch</a>
        </body></html>"#;
        let parsed = parse(body);
        assert_eq!(
            parsed
                .get("torch-2.12.1-cp312-cp312-win_amd64.whl")
                .and_then(Clone::clone),
            None
        );
    }
}
