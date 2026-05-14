use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynamicToolStatus {
    Available,
    Planned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicToolDescriptor {
    pub key: String,
    pub display_name: String,
    pub description: String,
    pub compatible_backends: Vec<String>,
    pub status: DynamicToolStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DynamicToolRegistry {
    tools: Vec<DynamicToolDescriptor>,
}

impl DynamicToolRegistry {
    pub fn with_builtin_tools() -> Self {
        Self {
            tools: vec![linear_graphql_descriptor()],
        }
    }

    pub fn tools(&self) -> &[DynamicToolDescriptor] {
        &self.tools
    }

    pub fn get(&self, key: &str) -> Option<&DynamicToolDescriptor> {
        self.tools.iter().find(|tool| tool.key == key)
    }

    pub fn compatible_with_backend(&self, backend: &str) -> Vec<&DynamicToolDescriptor> {
        self.tools
            .iter()
            .filter(|tool| {
                tool.compatible_backends
                    .iter()
                    .any(|candidate| candidate == backend)
            })
            .collect()
    }
}

pub fn linear_graphql_descriptor() -> DynamicToolDescriptor {
    DynamicToolDescriptor {
        key: "linear_graphql".into(),
        display_name: "Linear GraphQL".into(),
        description: "Planned client-side Linear GraphQL dynamic tool for Codex app-server parity."
            .into(),
        compatible_backends: vec!["codex".into()],
        status: DynamicToolStatus::Planned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_describes_linear_graphql_as_planned_codex_tool() {
        let registry = DynamicToolRegistry::with_builtin_tools();

        let tool = registry.get("linear_graphql").unwrap();
        assert_eq!(tool.display_name, "Linear GraphQL");
        assert_eq!(tool.status, DynamicToolStatus::Planned);
        assert_eq!(tool.compatible_backends, vec!["codex"]);
    }

    #[test]
    fn registry_filters_tools_by_backend() {
        let registry = DynamicToolRegistry::with_builtin_tools();

        assert_eq!(
            registry.compatible_with_backend("codex")[0].key,
            "linear_graphql"
        );
        assert!(registry.compatible_with_backend("claude-code").is_empty());
        assert!(registry.compatible_with_backend("dry-run").is_empty());
    }

    #[test]
    fn registry_returns_none_for_unknown_tool() {
        let registry = DynamicToolRegistry::with_builtin_tools();

        assert!(registry.get("unknown").is_none());
    }
}
