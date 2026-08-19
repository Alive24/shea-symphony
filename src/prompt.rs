use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::model::TrackerIssue;

pub const STRICT_LIQUID_RENDERER_MODE: &str = "strict-liquid-compatible";

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("unknown template variable: {0}")]
    UnknownVariable(String),
    #[error("template parse error: {0}")]
    Parse(String),
    #[error("template render error: {0}")]
    Render(String),
    #[error("template context error: {0}")]
    Context(String),
}

pub fn render_prompt(
    template: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let context = liquid_context(&serde_json::json!({
        "issue": issue,
        "attempt": attempt,
    }))?;
    render_strict_liquid(template, &context)
}

pub fn render_template_with_values(
    template: &str,
    values: &[(&str, String)],
) -> Result<String, PromptError> {
    let mut object = serde_json::Map::new();
    for (key, value) in values {
        object.insert((*key).to_string(), Value::String(value.clone()));
    }
    let context = liquid_context(&Value::Object(object))?;
    render_strict_liquid(template, &context)
}

/// Render a repository-owned Liquid template from a typed JSON context.
///
/// External variables remain strict: every referenced path must exist in
/// `values`, while Liquid-created locals continue to work normally. This is
/// the shared boundary for templates that need booleans, arrays, or nested
/// deterministic facts instead of the flat string context used by workpads.
pub fn render_template_with_json(template: &str, values: &Value) -> Result<String, PromptError> {
    let context = liquid_context(values)?;
    render_strict_liquid(template, &context)
}

pub fn smoke_render_prompt(template: &str, issue: &TrackerIssue) -> Result<(), PromptError> {
    render_prompt(template, issue, Some(1)).map(|_| ())
}

pub fn smoke_render_template(template: &str, values: &[(&str, String)]) -> Result<(), PromptError> {
    render_template_with_values(template, values).map(|_| ())
}

/// Smoke-render a repository-owned Liquid template with typed JSON values.
pub fn smoke_render_template_with_json(template: &str, values: &Value) -> Result<(), PromptError> {
    render_template_with_json(template, values).map(|_| ())
}

pub fn render_strict_liquid(
    template: &str,
    context: &liquid::Object,
) -> Result<String, PromptError> {
    validate_external_variables(template, &context_to_json(context)?)?;
    let parser = liquid::ParserBuilder::with_stdlib()
        .build()
        .map_err(|error| PromptError::Parse(error.to_string()))?;
    let parsed = parser
        .parse(template)
        .map_err(|error| PromptError::Parse(error.to_string()))?;
    parsed
        .render(context)
        .map(|rendered| rendered.trim_end().to_string())
        .map_err(|error| PromptError::Render(error.to_string()))
}

fn liquid_context<T: Serialize>(value: &T) -> Result<liquid::Object, PromptError> {
    liquid::to_object(value).map_err(|error| PromptError::Context(error.to_string()))
}

fn context_to_json(context: &liquid::Object) -> Result<Value, PromptError> {
    serde_json::to_value(context).map_err(|error| PromptError::Context(error.to_string()))
}

fn validate_external_variables(template: &str, context: &Value) -> Result<(), PromptError> {
    let mut locals = BTreeSet::from(["forloop".to_string(), "tablerowloop".to_string()]);
    let mut rest = template;

    loop {
        let variable_start = rest.find("{{");
        let tag_start = rest.find("{%");
        let Some((start, marker)) = earliest_marker(variable_start, tag_start) else {
            break;
        };
        let close = if marker == "{{" { "}}" } else { "%}" };
        let body_start = start + marker.len();
        let Some(body_end) = rest[body_start..]
            .find(close)
            .map(|index| body_start + index)
        else {
            return Err(PromptError::Parse(rest[start..].to_string()));
        };
        let body = rest[body_start..body_end].trim();
        if marker == "{{" {
            validate_expression_variables(body, context, &locals)?;
        } else {
            validate_tag_variables(body, context, &mut locals)?;
        }
        rest = &rest[body_end + close.len()..];
    }

    Ok(())
}

fn earliest_marker(
    variable_start: Option<usize>,
    tag_start: Option<usize>,
) -> Option<(usize, &'static str)> {
    match (variable_start, tag_start) {
        (Some(variable), Some(tag)) if variable < tag => Some((variable, "{{")),
        (Some(_), Some(tag)) => Some((tag, "{%")),
        (Some(variable), None) => Some((variable, "{{")),
        (None, Some(tag)) => Some((tag, "{%")),
        (None, None) => None,
    }
}

fn validate_tag_variables(
    tag: &str,
    context: &Value,
    locals: &mut BTreeSet<String>,
) -> Result<(), PromptError> {
    let mut parts = tag.split_whitespace();
    match parts.next() {
        Some("for") | Some("tablerow") => {
            if let Some(local) = parts.next() {
                locals.insert(local.trim().to_string());
            }
            if let Some(index) = tag.find(" in ") {
                validate_expression_variables(&tag[index + 4..], context, locals)?;
            }
        }
        Some("assign") => {
            if let Some(local) = parts.next() {
                locals.insert(local.trim_end_matches('=').to_string());
            }
            if let Some(index) = tag.find('=') {
                validate_expression_variables(&tag[index + 1..], context, locals)?;
            }
        }
        Some("capture") => {
            if let Some(local) = parts.next() {
                locals.insert(local.to_string());
            }
        }
        Some("if") | Some("unless") | Some("elsif") | Some("case") | Some("when") => {
            if let Some(index) = tag.find(char::is_whitespace) {
                validate_expression_variables(&tag[index..], context, locals)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_expression_variables(
    expression: &str,
    context: &Value,
    locals: &BTreeSet<String>,
) -> Result<(), PromptError> {
    let expression = strip_quoted_segments(expression);
    let bytes = expression.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !is_identifier_start(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len() && is_identifier_continue(bytes[index]) {
            index += 1;
        }
        let token = &expression[start..index];
        if should_skip_token(token, &expression, start, locals) {
            continue;
        }
        validate_variable_path(token, context, locals)?;
    }
    Ok(())
}

fn strip_quoted_segments(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            output.push(' ');
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            output.push(' ');
        } else {
            output.push(ch);
        }
    }
    output
}

fn should_skip_token(
    token: &str,
    expression: &str,
    start: usize,
    locals: &BTreeSet<String>,
) -> bool {
    if matches!(
        token,
        "and"
            | "or"
            | "contains"
            | "in"
            | "true"
            | "false"
            | "nil"
            | "null"
            | "blank"
            | "empty"
            | "limit"
            | "offset"
            | "reversed"
    ) {
        return true;
    }
    if locals.contains(token.split('.').next().unwrap_or(token)) {
        return true;
    }
    expression[..start].trim_end().ends_with('|')
}

fn validate_variable_path(
    token: &str,
    context: &Value,
    locals: &BTreeSet<String>,
) -> Result<(), PromptError> {
    let Some(root) = token.split('.').next() else {
        return Ok(());
    };
    if locals.contains(root) {
        return Ok(());
    }
    let mut current = context;
    for part in token.split('.') {
        match current {
            Value::Object(map) => {
                current = map
                    .get(part)
                    .ok_or_else(|| PromptError::UnknownVariable(token.to_string()))?;
            }
            _ => return Err(PromptError::UnknownVariable(token.to_string())),
        }
    }
    Ok(())
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackerIssue;

    fn issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "1".into(),
            item_id: None,
            identifier: "#1".into(),
            title: "Title".into(),
            description: Some("Body".into()),
            url: None,
            state: "Todo".into(),
            labels: vec!["rust".into(), "liquid".into()],
            assignees: vec![],
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn renders_known_variables_strictly() {
        let rendered = render_prompt(
            "Work on {{ issue.identifier }}: {{ issue.title }}",
            &issue(),
            None,
        )
        .unwrap();
        assert_eq!(rendered, "Work on #1: Title");
    }

    #[test]
    fn rejects_unknown_variables() {
        assert!(matches!(
            render_prompt("{{ issue.nope }}", &issue(), None).unwrap_err(),
            PromptError::UnknownVariable(variable) if variable == "issue.nope"
        ));
    }

    #[test]
    fn supports_basic_if_else() {
        let rendered = render_prompt(
            "{% if issue.description %}{{ issue.description }}{% else %}none{% endif %}",
            &issue(),
            None,
        )
        .unwrap();
        assert_eq!(rendered, "Body");
    }

    #[test]
    fn renders_attempt_and_falsey_conditionals() {
        let mut issue = issue();
        issue.description = None;

        let rendered = render_prompt(
            "attempt={{ attempt }} {% if issue.description %}body{% else %}missing{% endif %}",
            &issue,
            Some(3),
        )
        .unwrap();

        assert_eq!(rendered, "attempt=3 missing");
    }

    #[test]
    fn renders_non_string_issue_fields_with_liquid_semantics() {
        let rendered =
            render_prompt("labels={{ issue.labels | join: ', ' }}", &issue(), None).unwrap();

        assert_eq!(rendered, "labels=rust, liquid");
    }

    #[test]
    fn supports_liquid_loops_and_loop_locals() {
        let rendered = render_prompt(
            "{% for label in issue.labels %}[{{ forloop.index }}:{{ label }}]{% endfor %}",
            &issue(),
            None,
        )
        .unwrap();

        assert_eq!(rendered, "[1:rust][2:liquid]");
    }

    #[test]
    fn supports_default_join_and_size_filters() {
        let rendered = render_prompt(
            "title={{ issue.title | default: 'missing' }} labels={{ issue.labels | join: '/' }} count={{ issue.labels | size }}",
            &issue(),
            None,
        )
        .unwrap();

        assert_eq!(rendered, "title=Title labels=rust/liquid count=2");
    }

    #[test]
    fn rejects_unknown_filters() {
        assert!(matches!(
            render_prompt("{{ issue.title | nope }}", &issue(), None).unwrap_err(),
            PromptError::Parse(_)
        ));
    }

    #[test]
    fn rejects_malformed_templates() {
        assert!(matches!(
            render_prompt("{% if issue.title %}missing endif", &issue(), None).unwrap_err(),
            PromptError::Parse(_)
        ));
    }

    #[test]
    fn renders_typed_json_booleans_and_arrays() {
        let rendered = render_template_with_json(
            "{% if enabled %}{{ values | join: ',' }}{% endif %}",
            &serde_json::json!({"enabled": true, "values": ["a", "b"]}),
        )
        .unwrap();

        assert_eq!(rendered, "a,b");
    }
}
