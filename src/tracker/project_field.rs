use super::TrackerError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFieldAssignment {
    pub name: String,
    pub value: String,
}

impl ProjectFieldAssignment {
    pub fn parse(raw: &str) -> Result<Self, TrackerError> {
        let Some((name, value)) = raw.split_once('=') else {
            return Err(TrackerError::Payload(format!(
                "Project field assignment {raw:?} must use NAME=VALUE"
            )));
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            return Err(TrackerError::Payload(format!(
                "Project field assignment {raw:?} must include non-empty name and value"
            )));
        }

        Ok(Self {
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}
