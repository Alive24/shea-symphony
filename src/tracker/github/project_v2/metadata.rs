use std::cell::RefCell;

use crate::tracker::TrackerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tracker) enum ProjectV2OwnerType {
    Organization,
    User,
}

impl ProjectV2OwnerType {
    pub(in crate::tracker) fn parse(value: &str) -> Result<Self, TrackerError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "organization" => Ok(Self::Organization),
            "user" => Ok(Self::User),
            other => Err(TrackerError::IntegrationUnavailable(format!(
                "tracker.project_owner_type must be user or organization; got {other}"
            ))),
        }
    }

    pub(in crate::tracker) fn query_field(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::User => "user",
        }
    }

    pub(in crate::tracker) fn as_str(self) -> &'static str {
        self.query_field()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tracker) struct ProjectMetadata {
    pub(in crate::tracker) owner_type: ProjectV2OwnerType,
    pub(in crate::tracker) project_id: String,
    pub(in crate::tracker) status_field_id: String,
    pub(in crate::tracker) status_options: Vec<(String, String)>,
    pub(in crate::tracker) fields: Vec<ProjectFieldMetadata>,
}

impl ProjectMetadata {
    pub(in crate::tracker) fn field(&self, name: &str) -> Option<&ProjectFieldMetadata> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub(in crate::tracker) fn status_field(&self) -> ProjectFieldMetadata {
        if let Some(field) = self
            .fields
            .iter()
            .find(|field| field.id == self.status_field_id)
        {
            return field.clone();
        }

        ProjectFieldMetadata {
            id: self.status_field_id.clone(),
            name: "Status".into(),
            kind: ProjectFieldKind::SingleSelect,
            options: self.status_options.clone(),
            rest_id: self
                .fields
                .iter()
                .find(|field| field.id == self.status_field_id)
                .and_then(|field| field.rest_id),
        }
    }

    pub(in crate::tracker) fn supported_rest_field_ids(&self) -> Vec<u64> {
        self.fields
            .iter()
            .filter(|field| field.kind.supports_rest_update())
            .filter_map(|field| field.rest_id)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tracker) struct ProjectFieldMetadata {
    pub(in crate::tracker) id: String,
    pub(in crate::tracker) name: String,
    pub(in crate::tracker) kind: ProjectFieldKind,
    pub(in crate::tracker) options: Vec<(String, String)>,
    pub(in crate::tracker) rest_id: Option<u64>,
}

impl ProjectFieldMetadata {
    pub(in crate::tracker) fn option_id(&self, option_name: &str) -> Option<String> {
        self.options
            .iter()
            .find_map(|(id, name)| (name == option_name).then_some(id.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tracker) enum ProjectFieldKind {
    SingleSelect,
    Text,
    Number,
    Date,
    Iteration,
    Unknown,
}

impl ProjectFieldKind {
    fn supports_rest_update(self) -> bool {
        matches!(
            self,
            Self::SingleSelect | Self::Text | Self::Number | Self::Date
        )
    }
}

#[derive(Debug, Clone, Default)]
pub(in crate::tracker) struct ProjectMetadataCache {
    value: RefCell<Option<ProjectMetadata>>,
}

impl ProjectMetadataCache {
    pub(in crate::tracker) fn get_or_try_init<F>(
        &self,
        fetch: F,
    ) -> Result<ProjectMetadata, TrackerError>
    where
        F: FnOnce() -> Result<ProjectMetadata, TrackerError>,
    {
        if let Some(metadata) = self.value.borrow().clone() {
            return Ok(metadata);
        }

        let metadata = fetch()?;
        *self.value.borrow_mut() = Some(metadata.clone());
        Ok(metadata)
    }

    pub(in crate::tracker) fn refresh<F>(&self, fetch: F) -> Result<ProjectMetadata, TrackerError>
    where
        F: FnOnce() -> Result<ProjectMetadata, TrackerError>,
    {
        let metadata = fetch()?;
        *self.value.borrow_mut() = Some(metadata.clone());
        Ok(metadata)
    }
}

pub(in crate::tracker) fn project_field_from_metadata_with_refresh<F>(
    cache: &ProjectMetadataCache,
    field_name: &str,
    fetch: F,
) -> Result<(ProjectMetadata, ProjectFieldMetadata), TrackerError>
where
    F: Fn() -> Result<ProjectMetadata, TrackerError>,
{
    let metadata = cache.get_or_try_init(&fetch)?;
    if let Some(field) = metadata.field(field_name) {
        return Ok((metadata.clone(), field.clone()));
    }

    let refreshed = cache.refresh(fetch)?;
    let field = refreshed.field(field_name).cloned().ok_or_else(|| {
        TrackerError::IntegrationUnavailable(format!(
            "ProjectV2 field {field_name:?} was not found after metadata refresh"
        ))
    })?;
    Ok((refreshed, field))
}

#[cfg(test)]
pub(in crate::tracker) fn status_option_id(
    metadata: &ProjectMetadata,
    option_name: &str,
    status_field: &str,
) -> Result<String, TrackerError> {
    metadata
        .status_options
        .iter()
        .find_map(|(id, name)| (name == option_name).then_some(id.clone()))
        .ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "status option {option_name:?} was not found in ProjectV2 field {status_field}"
            ))
        })
}

pub(in crate::tracker) fn project_metadata_from_response(
    response: &serde_json::Value,
    status_field: &str,
) -> Result<ProjectMetadata, TrackerError> {
    let (owner_type, project) = response
        .pointer("/data/organization/projectV2")
        .map(|project| (ProjectV2OwnerType::Organization, project))
        .or_else(|| {
            response
                .pointer("/data/user/projectV2")
                .map(|project| (ProjectV2OwnerType::User, project))
        })
        .ok_or_else(|| TrackerError::Payload("missing ProjectV2 metadata payload".into()))?;
    let project_id = project
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| TrackerError::Payload("ProjectV2 metadata missing project id".into()))?;
    let fields = project
        .pointer("/fields/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TrackerError::Payload("ProjectV2 metadata missing fields".into()))?;

    let fields = fields
        .iter()
        .filter_map(project_field_metadata)
        .collect::<Vec<_>>();

    if let Some(status_field_metadata) = fields.iter().find(|field| field.name == status_field) {
        return Ok(ProjectMetadata {
            owner_type,
            project_id: project_id.to_string(),
            status_field_id: status_field_metadata.id.clone(),
            status_options: status_field_metadata.options.clone(),
            fields,
        });
    }

    Err(TrackerError::Payload(format!(
        "ProjectV2 status field {status_field:?} was not found"
    )))
}

fn project_field_metadata(field: &serde_json::Value) -> Option<ProjectFieldMetadata> {
    let id = field.get("id")?.as_str()?.to_string();
    let name = field.get("name")?.as_str()?.to_string();
    let kind = match field
        .get("__typename")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
    {
        "ProjectV2SingleSelectField" => ProjectFieldKind::SingleSelect,
        "ProjectV2Field" => ProjectFieldKind::Text,
        "ProjectV2IterationField" => ProjectFieldKind::Iteration,
        "ProjectV2FieldCommon" => ProjectFieldKind::Unknown,
        "ProjectV2NumberField" => ProjectFieldKind::Number,
        "ProjectV2DateField" => ProjectFieldKind::Date,
        _ => ProjectFieldKind::Unknown,
    };
    let options = field
        .get("options")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some((
                option.get("id")?.as_str()?.to_string(),
                option.get("name")?.as_str()?.to_string(),
            ))
        })
        .collect();

    Some(ProjectFieldMetadata {
        id,
        name,
        kind,
        options,
        rest_id: None,
    })
}
