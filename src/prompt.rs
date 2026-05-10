use serde_json::Value;
use thiserror::Error;

use crate::model::TrackerIssue;

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("unknown template variable: {0}")]
    UnknownVariable(String),
    #[error("unsupported template tag: {0}")]
    UnsupportedTag(String),
}

pub fn render_prompt(
    template: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let template = render_conditionals(template, issue, attempt)?;
    render_variables(&template, issue, attempt)
}

fn render_conditionals(
    template: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let mut output = String::new();
    let mut rest = template;

    while let Some(start) = rest.find("{%") {
        output.push_str(&rest[..start]);
        let tag_end = rest[start + 2..]
            .find("%}")
            .ok_or_else(|| PromptError::UnsupportedTag(rest[start..].to_string()))?
            + start
            + 2;
        let tag = rest[start + 2..tag_end].trim();
        if let Some(condition) = tag.strip_prefix("if ") {
            let after_tag = &rest[tag_end + 2..];
            let endif = after_tag
                .find("{% endif %}")
                .ok_or_else(|| PromptError::UnsupportedTag(tag.to_string()))?;
            let block = &after_tag[..endif];
            let after_block = &after_tag[endif + "{% endif %}".len()..];
            let (truthy_block, falsey_block) = split_else(block);
            output.push_str(if lookup_truthy(condition.trim(), issue, attempt)? {
                truthy_block
            } else {
                falsey_block.unwrap_or("")
            });
            rest = after_block;
        } else {
            return Err(PromptError::UnsupportedTag(tag.to_string()));
        }
    }

    output.push_str(rest);
    Ok(output)
}

fn split_else(block: &str) -> (&str, Option<&str>) {
    if let Some(index) = block.find("{% else %}") {
        (&block[..index], Some(&block[index + "{% else %}".len()..]))
    } else {
        (block, None)
    }
}

fn render_variables(
    template: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let mut output = String::new();
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let end = rest[start + 2..]
            .find("}}")
            .ok_or_else(|| PromptError::UnknownVariable(rest[start..].to_string()))?
            + start
            + 2;
        let variable = rest[start + 2..end].trim();
        output.push_str(&lookup_string(variable, issue, attempt)?);
        rest = &rest[end + 2..];
    }

    output.push_str(rest);
    Ok(output)
}

fn lookup_truthy(
    variable: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<bool, PromptError> {
    match lookup_value(variable, issue, attempt)? {
        Value::Null => Ok(false),
        Value::Bool(value) => Ok(value),
        Value::String(value) => Ok(!value.is_empty()),
        Value::Array(value) => Ok(!value.is_empty()),
        Value::Object(value) => Ok(!value.is_empty()),
        Value::Number(value) => Ok(value.as_i64().unwrap_or(0) != 0),
    }
}

fn lookup_string(
    variable: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let value = lookup_value(variable, issue, attempt)?;
    Ok(match value {
        Value::Null => String::new(),
        Value::String(value) => value,
        other => other.to_string(),
    })
}

fn lookup_value(
    variable: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
) -> Result<Value, PromptError> {
    if variable == "attempt" {
        return Ok(attempt.map(Value::from).unwrap_or(Value::Null));
    }

    let Some(path) = variable.strip_prefix("issue.") else {
        return Err(PromptError::UnknownVariable(variable.to_string()));
    };

    let value = serde_json::to_value(issue).expect("TrackerIssue serializes");
    path.split('.')
        .try_fold(value, |current, part| match current {
            Value::Object(map) => map
                .get(part)
                .cloned()
                .ok_or_else(|| PromptError::UnknownVariable(variable.to_string())),
            _ => Err(PromptError::UnknownVariable(variable.to_string())),
        })
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
            labels: vec!["rust".into()],
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
        assert!(render_prompt("{{ issue.nope }}", &issue(), None).is_err());
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
}
