use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneClaimLane {
    Main,
    Review,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneClaimActor {
    Codex,
    Gemini,
    Antigravity,
    Claude,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneClaimSource {
    Loop,
    Manual,
    Goal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneClaimState {
    Active,
    Done,
    Stale,
    Failed,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneClaim {
    pub lane: LaneClaimLane,
    pub actor: LaneClaimActor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    pub source: LaneClaimSource,
    pub issue: String,
    pub run: String,
    pub state: LaneClaimState,
    pub thread: String,
    pub registry: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneClaimParseError {
    MissingKey(&'static str),
    UnsupportedVersion(String),
    InvalidToken(String),
    InvalidValue { key: &'static str, value: String },
}

impl std::fmt::Display for LaneClaimParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey(key) => write!(formatter, "missing claim key `{key}`"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported claim version `{version}`")
            }
            Self::InvalidToken(token) => write!(formatter, "invalid claim token `{token}`"),
            Self::InvalidValue { key, value } => {
                write!(formatter, "invalid claim value `{key}={value}`")
            }
        }
    }
}

impl std::error::Error for LaneClaimParseError {}

impl LaneClaimLane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Review => "review",
            Self::Merge => "merge",
        }
    }
}

impl LaneClaimActor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Antigravity => "antigravity",
            Self::Claude => "claude",
            Self::Human => "human",
        }
    }
}

impl LaneClaimSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::Manual => "manual",
            Self::Goal => "goal",
        }
    }
}

impl LaneClaimState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Stale => "stale",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }

    pub fn is_terminal_audit_pointer(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Stale | Self::Failed | Self::Superseded
        )
    }
}

impl LaneClaim {
    pub fn active(
        issue: &str,
        lane: LaneClaimLane,
        actor: LaneClaimActor,
        source: LaneClaimSource,
        now_ms: u64,
    ) -> Self {
        let run = generate_run_id(now_ms, issue, lane);
        Self {
            lane,
            actor,
            worker: None,
            source,
            issue: issue.to_string(),
            registry: format!("run/{run}"),
            run,
            state: LaneClaimState::Active,
            thread: "unknown".into(),
        }
    }

    pub fn with_state(&self, state: LaneClaimState) -> Self {
        Self {
            state,
            ..self.clone()
        }
    }

    pub fn with_worker(&self, worker: impl Into<String>) -> Self {
        let worker = worker.into();
        Self {
            worker: (!worker.trim().is_empty()).then(|| worker.trim().to_string()),
            ..self.clone()
        }
    }

    pub fn render(&self) -> String {
        let mut tokens = vec![
            "v=1".to_string(),
            claim_token("lane", self.lane.as_str()),
            claim_token("actor", self.actor.as_str()),
        ];
        if let Some(worker) = self.worker.as_deref() {
            tokens.push(claim_token("worker", worker));
        }
        tokens.extend([
            claim_token("source", self.source.as_str()),
            claim_token("issue", &self.issue),
            claim_token("run", &self.run),
            claim_token("state", self.state.as_str()),
            claim_token("thread", &self.thread),
            claim_token("registry", &self.registry),
        ]);
        tokens.join(" ")
    }

    pub fn parse(input: &str) -> Result<Self, LaneClaimParseError> {
        let mut values = BTreeMap::new();
        for (key, value) in parse_claim_tokens(input)? {
            values.insert(key, value);
        }

        let version = required(&values, "v")?;
        if version != "1" {
            return Err(LaneClaimParseError::UnsupportedVersion(version.into()));
        }

        Ok(Self {
            lane: parse_lane(required(&values, "lane")?)?,
            actor: parse_actor(required(&values, "actor")?)?,
            worker: values.get("worker").map(|value| value.to_string()),
            source: parse_source(required(&values, "source")?)?,
            issue: required(&values, "issue")?.to_string(),
            run: required(&values, "run")?.to_string(),
            state: parse_state(required(&values, "state")?)?,
            thread: required(&values, "thread")?.to_string(),
            registry: required(&values, "registry")?.to_string(),
        })
    }
}

fn required<'a>(
    values: &'a BTreeMap<String, String>,
    key: &'static str,
) -> Result<&'a str, LaneClaimParseError> {
    values
        .get(key)
        .map(String::as_str)
        .ok_or(LaneClaimParseError::MissingKey(key))
}

fn claim_token(key: &str, value: &str) -> String {
    format!("{key}={}", render_claim_value(value))
}

fn render_claim_value(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '"' | '\\'))
    {
        let mut rendered = String::with_capacity(value.len() + 2);
        rendered.push('"');
        for character in value.chars() {
            match character {
                '\\' => rendered.push_str("\\\\"),
                '"' => rendered.push_str("\\\""),
                '\n' => rendered.push_str("\\n"),
                '\r' => rendered.push_str("\\r"),
                '\t' => rendered.push_str("\\t"),
                other => rendered.push(other),
            }
        }
        rendered.push('"');
        rendered
    } else {
        value.to_string()
    }
}

fn parse_claim_tokens(input: &str) -> Result<Vec<(String, String)>, LaneClaimParseError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((_, character)) = chars.peek().copied() {
        if character.is_whitespace() {
            chars.next();
            continue;
        }

        let token_start = chars.peek().map(|(index, _)| *index).unwrap_or(input.len());
        let mut key = String::new();
        while let Some((_, character)) = chars.peek().copied() {
            if character == '=' {
                chars.next();
                break;
            }
            if character.is_whitespace() {
                return Err(invalid_token_from(input, token_start, chars.peek()));
            }
            key.push(character);
            chars.next();
        }

        if key.is_empty() {
            return Err(invalid_token_from(input, token_start, chars.peek()));
        }

        let Some((_, next)) = chars.peek().copied() else {
            return Err(LaneClaimParseError::InvalidToken(key));
        };

        let value = if next == '"' {
            chars.next();
            parse_quoted_claim_value(input, token_start, &mut chars)?
        } else {
            parse_unquoted_claim_value(&mut chars)
        };

        if value.is_empty() {
            return Err(LaneClaimParseError::InvalidToken(format!("{key}=")));
        }

        tokens.push((key, value));
    }

    Ok(tokens)
}

fn parse_quoted_claim_value(
    input: &str,
    token_start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<String, LaneClaimParseError> {
    let mut value = String::new();
    let mut closed = false;

    while let Some((_, character)) = chars.next() {
        match character {
            '"' => {
                closed = true;
                break;
            }
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    return Err(invalid_token_slice(input, token_start, input.len()));
                };
                match escaped {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    other => value.push(other),
                }
            }
            other => value.push(other),
        }
    }

    if !closed {
        return Err(invalid_token_slice(input, token_start, input.len()));
    }

    if let Some((_, character)) = chars.peek().copied() {
        if !character.is_whitespace() {
            return Err(invalid_token_from(input, token_start, chars.peek()));
        }
    }

    Ok(value)
}

fn parse_unquoted_claim_value(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> String {
    let mut value = String::new();
    while let Some((_, character)) = chars.peek().copied() {
        if character.is_whitespace() {
            break;
        }
        value.push(character);
        chars.next();
    }
    value
}

fn invalid_token_from(
    input: &str,
    token_start: usize,
    cursor: Option<&(usize, char)>,
) -> LaneClaimParseError {
    let token_end = cursor.map(|(index, _)| *index).unwrap_or(input.len());
    invalid_token_slice(input, token_start, token_end)
}

fn invalid_token_slice(input: &str, token_start: usize, token_end: usize) -> LaneClaimParseError {
    LaneClaimParseError::InvalidToken(input[token_start..token_end].into())
}

fn parse_lane(value: &str) -> Result<LaneClaimLane, LaneClaimParseError> {
    match value {
        "main" => Ok(LaneClaimLane::Main),
        "review" => Ok(LaneClaimLane::Review),
        "merge" => Ok(LaneClaimLane::Merge),
        other => Err(invalid("lane", other)),
    }
}

fn parse_actor(value: &str) -> Result<LaneClaimActor, LaneClaimParseError> {
    match value {
        "codex" => Ok(LaneClaimActor::Codex),
        "gemini" => Ok(LaneClaimActor::Gemini),
        "antigravity" => Ok(LaneClaimActor::Antigravity),
        "claude" => Ok(LaneClaimActor::Claude),
        "human" => Ok(LaneClaimActor::Human),
        other => Err(invalid("actor", other)),
    }
}

fn parse_source(value: &str) -> Result<LaneClaimSource, LaneClaimParseError> {
    match value {
        "loop" => Ok(LaneClaimSource::Loop),
        "manual" => Ok(LaneClaimSource::Manual),
        "goal" => Ok(LaneClaimSource::Goal),
        other => Err(invalid("source", other)),
    }
}

fn parse_state(value: &str) -> Result<LaneClaimState, LaneClaimParseError> {
    match value {
        "active" => Ok(LaneClaimState::Active),
        "done" => Ok(LaneClaimState::Done),
        "stale" => Ok(LaneClaimState::Stale),
        "failed" => Ok(LaneClaimState::Failed),
        "superseded" => Ok(LaneClaimState::Superseded),
        other => Err(invalid("state", other)),
    }
}

fn invalid(key: &'static str, value: &str) -> LaneClaimParseError {
    LaneClaimParseError::InvalidValue {
        key,
        value: value.into(),
    }
}

pub fn generate_run_id(now_ms: u64, issue: &str, lane: LaneClaimLane) -> String {
    let seconds = now_ms / 1000;
    let suffix = (now_ms & 0xffff) as u16;
    format!(
        "{}-issue{}-{}-{suffix:04x}",
        format_utc_compact(seconds),
        issue.trim().trim_start_matches('#'),
        lane.as_str()
    )
}

fn format_utc_compact(seconds_since_unix_epoch: u64) -> String {
    let days = (seconds_since_unix_epoch / 86_400) as i64;
    let seconds_of_day = seconds_since_unix_epoch % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_unix_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_and_parses_v1_claim() {
        let claim = LaneClaim::active(
            "#244",
            LaneClaimLane::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Loop,
            1_778_904_900_123,
        );

        let rendered = claim.render();
        assert!(rendered.starts_with("v=1 lane=main actor=codex source=loop issue=#244"));
        assert_eq!(LaneClaim::parse(&rendered).unwrap(), claim);
    }

    #[test]
    fn renders_and_parses_worker_identity_when_present() {
        let claim = LaneClaim::active(
            "#265",
            LaneClaimLane::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            1_778_904_900_123,
        )
        .with_worker("codex-manual-main");

        let rendered = claim.render();
        assert!(rendered.contains(" worker=codex-manual-main "));
        assert_eq!(LaneClaim::parse(&rendered).unwrap(), claim);
    }

    #[test]
    fn renders_and_parses_quoted_worker_display_label() {
        let claim = LaneClaim::active(
            "#297",
            LaneClaimLane::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            1_778_904_900_123,
        )
        .with_worker("Codex Manual Main");

        let rendered = claim.render();

        assert!(rendered.contains(" worker=\"Codex Manual Main\" "));
        assert_eq!(LaneClaim::parse(&rendered).unwrap(), claim);
    }

    #[test]
    fn renders_and_parses_escaped_worker_display_label() {
        let claim = LaneClaim::active(
            "#297",
            LaneClaimLane::Review,
            LaneClaimActor::Gemini,
            LaneClaimSource::Manual,
            1_778_904_900_123,
        )
        .with_worker("Manual \"Gemini\" Review\\A");

        let rendered = claim.render();

        assert!(rendered.contains(" worker=\"Manual \\\"Gemini\\\" Review\\\\A\" "));
        assert_eq!(LaneClaim::parse(&rendered).unwrap(), claim);
    }

    #[test]
    fn parses_existing_unquoted_claim_values() {
        let claim = LaneClaim::parse(
            "v=1 lane=main actor=codex worker=codex-manual-main source=manual issue=#297 run=20260518T0640Z-issue297-main-73cb state=active thread=unknown registry=run/20260518T0640Z-issue297-main-73cb",
        )
        .unwrap();

        assert_eq!(claim.worker.as_deref(), Some("codex-manual-main"));
        assert_eq!(claim.issue, "#297");
    }

    #[test]
    fn rejects_unclosed_quoted_claim_value() {
        let error = LaneClaim::parse(
            "v=1 lane=main actor=codex worker=\"Codex Manual Main source=manual issue=#297 run=run state=active thread=unknown registry=run/run",
        )
        .unwrap_err();

        assert!(matches!(error, LaneClaimParseError::InvalidToken(_)));
    }

    #[test]
    fn generated_run_id_is_human_readable_and_path_safe() {
        let run = generate_run_id(1_778_904_900_123, "#244", LaneClaimLane::Main);

        assert!(run.starts_with("20260516T"));
        assert!(run.contains("-issue244-main-"));
        assert!(!run.contains(' '));
    }

    #[test]
    fn rejects_legacy_free_text() {
        assert!(LaneClaim::parse("Gemini A").is_err());
    }
}
