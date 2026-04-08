use tracing_subscriber::registry::LookupSpan;

pub fn initialize() -> sentry::ClientInitGuard {
    sentry::init(
        "https://6817fd5af2a8b0d470faeaa0afabc59e@o4511181517815808.ingest.us.sentry.io/4511181521092608",
    )
}

pub fn tracing_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    sentry::integrations::tracing::layer()
}
