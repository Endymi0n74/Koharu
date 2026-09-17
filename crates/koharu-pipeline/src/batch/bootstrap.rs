//! Shared bootstrap helpers for the CLI binaries.

use std::time::Duration;

/// Initializes the ML runtimes, retrying because CUDA package staging can
/// transiently fail right after installation.
pub async fn initialize_with_retry() {
    let mut delay = Duration::from_secs(1);
    let mut attempt = 0_u64;
    loop {
        attempt += 1;
        match koharu_ml::init().await {
            Ok(()) => return,
            Err(error) => {
                let jitter = Duration::from_millis((attempt.wrapping_mul(137)) % 251);
                let wait = delay + jitter;
                eprintln!(
                    "runtime initialization attempt {attempt} failed: {error}; retrying in {:.1}s",
                    wait.as_secs_f64()
                );
                tokio::time::sleep(wait).await;
                delay = delay.saturating_mul(2).min(Duration::from_secs(30));
            }
        }
    }
}

/// MIME type inferred from a page file extension.
pub fn image_media_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types_follow_extensions() {
        assert_eq!(image_media_type(std::path::Path::new("a.jpg")), "image/jpeg");
        assert_eq!(image_media_type(std::path::Path::new("a.webp")), "image/webp");
        assert_eq!(image_media_type(std::path::Path::new("a.png")), "image/png");
        assert_eq!(image_media_type(std::path::Path::new("a")), "image/png");
    }
}
