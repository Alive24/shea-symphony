use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("tracker fixture failed: {0}")]
    Fixture(String),
    #[error("tracker payload failed: {0}")]
    Payload(String),
    #[error("tracker integration is unavailable: {0}")]
    IntegrationUnavailable(String),
    #[error("tracker operation is not implemented yet: {0}")]
    NotImplemented(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStateFailureKind {
    Auth,
    Network,
    TransientBackend,
    RateLimit,
    ResourceLimit,
    Schema,
    PartialResponse,
    Payload,
    MissingCapability,
    Unknown,
}

impl ProjectStateFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Network => "network",
            Self::TransientBackend => "transient_backend",
            Self::RateLimit => "rate_limit",
            Self::ResourceLimit => "resource_limit",
            Self::Schema => "schema",
            Self::PartialResponse => "partial_response",
            Self::Payload => "payload",
            Self::MissingCapability => "missing_capability",
            Self::Unknown => "unknown",
        }
    }
}

pub fn classify_project_state_error(error: &TrackerError) -> ProjectStateFailureKind {
    match error {
        TrackerError::Fixture(_) => ProjectStateFailureKind::Payload,
        TrackerError::Payload(message) => classify_project_state_failure_message(message),
        TrackerError::IntegrationUnavailable(message) => {
            classify_project_state_failure_message(message)
        }
        TrackerError::NotImplemented(_) => ProjectStateFailureKind::MissingCapability,
    }
}

pub fn classify_project_state_failure_message(message: &str) -> ProjectStateFailureKind {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("rate limit")
        || normalized.contains("secondary rate")
        || normalized.contains("too many requests")
        || normalized.contains("http 429")
    {
        ProjectStateFailureKind::RateLimit
    } else if normalized.contains("resource limit")
        || normalized.contains("resource limitation")
        || normalized.contains("maximum node limit")
        || normalized.contains("max node limit")
        || normalized.contains("exceeds maximum")
        || normalized.contains("query has complexity")
        || normalized.contains("query is too complex")
    {
        ProjectStateFailureKind::ResourceLimit
    } else if normalized.contains("authentication")
        || normalized.contains("authenticate")
        || normalized.contains("auth login")
        || normalized.contains("bad credentials")
        || normalized.contains("unauthorized")
        || normalized.contains("http 401")
        || normalized.contains("http 403")
    {
        ProjectStateFailureKind::Auth
    } else if normalized.contains("http 500")
        || normalized.contains("http 502")
        || normalized.contains("http 503")
        || normalized.contains("http 504")
        || normalized.contains("bad gateway")
        || normalized.contains("service unavailable")
        || normalized.contains("gateway timeout")
        || normalized.contains("internal server error")
    {
        ProjectStateFailureKind::TransientBackend
    } else if normalized.contains("could not resolve host")
        || normalized.contains("error connecting to")
        || normalized.contains("failed to connect")
        || normalized.contains("could not connect")
        || normalized.contains("connection timed out")
        || normalized.contains("timed out after")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
        || normalized.contains("connection closed")
        || normalized.contains("temporary failure in name resolution")
        || normalized.contains("no route to host")
        || normalized.contains("i/o timeout")
        || normalized.contains("context deadline exceeded")
        || is_transport_eof_message(&normalized)
        || normalized.contains("network")
        || normalized.contains("tls")
    {
        ProjectStateFailureKind::Network
    } else if normalized.contains("missing projectv2")
        || normalized.contains("partial projectv2")
        || normalized.contains("missing status field")
        || normalized.contains("missing fieldvalues")
        || normalized.contains("missing pageinfo")
    {
        ProjectStateFailureKind::PartialResponse
    } else if normalized.contains("could not resolve to a projectv2")
        || normalized.contains("field ")
        || normalized.contains("doesn't exist")
        || normalized.contains("schema")
    {
        ProjectStateFailureKind::Schema
    } else if normalized.contains("invalid gh graphql json")
        || normalized.contains("invalid github graphql json")
        || normalized.contains("invalid gh api json")
        || normalized.contains("invalid github api json")
    {
        ProjectStateFailureKind::Payload
    } else if normalized.contains("does not support")
        || normalized.contains("not implemented")
        || normalized.contains("missing cli capability")
        || normalized.contains("cli gap")
    {
        ProjectStateFailureKind::MissingCapability
    } else {
        ProjectStateFailureKind::Unknown
    }
}

fn is_transport_eof_message(normalized: &str) -> bool {
    let trimmed = normalized.trim();
    let eof_suffix = trimmed == "eof" || trimmed.ends_with(": eof") || trimmed.ends_with(" eof");
    if !eof_suffix {
        return false;
    }

    let looks_like_json_parse_error = normalized.contains("invalid gh")
        || normalized.contains("invalid github")
        || normalized.contains("while parsing");
    if looks_like_json_parse_error {
        return false;
    }

    normalized.contains("api.github.com")
        || normalized.contains("graphql")
        || normalized.contains("rest")
        || normalized.contains("http://")
        || normalized.contains("https://")
}
