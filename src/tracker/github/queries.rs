const GITHUB_PROJECT_ITEM_PAGE_SIZE: usize = 25;
const GITHUB_PROJECT_FIELD_VALUE_PAGE_SIZE: usize = 30;
const GITHUB_PROJECT_LABEL_PAGE_SIZE: usize = 25;
const GITHUB_PROJECT_ASSIGNEE_PAGE_SIZE: usize = 10;
const GITHUB_PROJECT_SUBISSUE_PAGE_SIZE: usize = 50;
const GITHUB_PROJECT_LINKED_PR_PAGE_SIZE: usize = 10;
const GITHUB_PROJECT_COMMENT_PAGE_SIZE: usize = 100;
const GITHUB_PROJECT_METADATA_FIELD_PAGE_SIZE: usize = 50;
const GITHUB_WORKPAD_COMMENT_PAGE_SIZE: usize = 50;
const GITHUB_ISSUE_PROJECT_ITEM_PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubProjectReadMode {
    QueueScan,
    RichEvidence,
}

pub(in crate::tracker) fn github_project_query(
    owner_field: &str,
    mode: GithubProjectReadMode,
) -> String {
    let rich_issue_fields = match mode {
        GithubProjectReadMode::QueueScan => String::new(),
        GithubProjectReadMode::RichEvidence => rich_issue_evidence_fields(),
    };
    format!(
        r#"
query SheaSymphonyProject($owner: String!, $number: Int!, $cursor: String) {{
  rateLimit {{
    cost
    remaining
    resetAt
  }}
  {owner_field}(login: $owner) {{
    projectV2(number: $number) {{
      items(first: {GITHUB_PROJECT_ITEM_PAGE_SIZE}, after: $cursor) {{
        pageInfo {{
          hasNextPage
          endCursor
        }}
        nodes {{
          id
          fieldValues(first: {GITHUB_PROJECT_FIELD_VALUE_PAGE_SIZE}) {{
            nodes {{
              ... on ProjectV2ItemFieldSingleSelectValue {{
                name
                field {{
                  ... on ProjectV2SingleSelectField {{
                    name
                  }}
                }}
              }}
              ... on ProjectV2ItemFieldTextValue {{
                text
                field {{
                  ... on ProjectV2FieldCommon {{
                    name
                  }}
                }}
              }}
              ... on ProjectV2ItemFieldNumberValue {{
                number
                field {{
                  ... on ProjectV2FieldCommon {{
                    name
                  }}
                }}
              }}
            }}
          }}
          content {{
            __typename
            ... on Issue {{
              id
              number
              title
              url
              state
              createdAt
              updatedAt
              labels(first: {GITHUB_PROJECT_LABEL_PAGE_SIZE}) {{
                nodes {{
                  name
                }}
              }}
              assignees(first: {GITHUB_PROJECT_ASSIGNEE_PAGE_SIZE}) {{
                nodes {{
                  login
                }}
              }}
              parent {{
                id
                number
                title
                state
                url
              }}
              subIssues(first: {GITHUB_PROJECT_SUBISSUE_PAGE_SIZE}) {{
                nodes {{
                  id
                  number
                  title
                  state
                  url
                }}
              }}
{rich_issue_fields}
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    )
}

fn rich_issue_evidence_fields() -> String {
    format!(
        r#"
              body
              closedByPullRequestsReferences(first: {GITHUB_PROJECT_LINKED_PR_PAGE_SIZE}) {{
                nodes {{
                  id
                  number
                  url
                  state
                  isDraft
                  baseRefName
                  headRefName
                }}
              }}
              comments(first: {GITHUB_PROJECT_COMMENT_PAGE_SIZE}) {{
                nodes {{
                  body
                }}
              }}
              recentComments: comments(last: {GITHUB_PROJECT_COMMENT_PAGE_SIZE}) {{
                nodes {{
                  body
                }}
              }}"#
    )
}

pub(in crate::tracker) fn github_issue_evidence_query() -> String {
    format!(
        r#"
query SheaSymphonyIssueEvidence($owner: String!, $repo: String!, $number: Int!) {{
  rateLimit {{
    cost
    remaining
    resetAt
  }}
  repository(owner: $owner, name: $repo) {{
    issue(number: $number) {{
      id
      number
      title
      url
      state
      createdAt
      updatedAt
      labels(first: {GITHUB_PROJECT_LABEL_PAGE_SIZE}) {{
        nodes {{
          name
        }}
      }}
      assignees(first: {GITHUB_PROJECT_ASSIGNEE_PAGE_SIZE}) {{
        nodes {{
          login
        }}
      }}
      parent {{
        id
        number
        title
        state
        url
      }}
      subIssues(first: {GITHUB_PROJECT_SUBISSUE_PAGE_SIZE}) {{
        nodes {{
          id
          number
          title
          state
          url
        }}
      }}
{}
    }}
  }}
}}
"#,
        rich_issue_evidence_fields()
    )
}

pub(in crate::tracker) fn github_issue_project_item_query() -> String {
    format!(
        r#"
query SheaSymphonyIssueProjectItem($owner: String!, $repo: String!, $number: Int!) {{
  rateLimit {{
    cost
    remaining
    resetAt
  }}
  repository(owner: $owner, name: $repo) {{
    issue(number: $number) {{
      __typename
      id
      number
      title
      body
      url
      state
      createdAt
      updatedAt
      labels(first: {GITHUB_PROJECT_LABEL_PAGE_SIZE}) {{
        nodes {{
          name
        }}
      }}
      assignees(first: {GITHUB_PROJECT_ASSIGNEE_PAGE_SIZE}) {{
        nodes {{
          login
        }}
      }}
      parent {{
        id
        number
        title
        state
        url
      }}
      subIssues(first: {GITHUB_PROJECT_SUBISSUE_PAGE_SIZE}) {{
        nodes {{
          id
          number
          title
          state
          url
        }}
      }}
      closedByPullRequestsReferences(first: {GITHUB_PROJECT_LINKED_PR_PAGE_SIZE}) {{
        nodes {{
          id
          number
          url
          state
          isDraft
          baseRefName
          headRefName
        }}
      }}
      comments(first: {GITHUB_PROJECT_COMMENT_PAGE_SIZE}) {{
        nodes {{
          body
        }}
      }}
      recentComments: comments(last: {GITHUB_PROJECT_COMMENT_PAGE_SIZE}) {{
        nodes {{
          body
        }}
      }}
      projectItems(first: {GITHUB_ISSUE_PROJECT_ITEM_PAGE_SIZE}) {{
        nodes {{
          id
          project {{
            number
          }}
          fieldValues(first: {GITHUB_PROJECT_FIELD_VALUE_PAGE_SIZE}) {{
            nodes {{
              ... on ProjectV2ItemFieldSingleSelectValue {{
                name
                field {{
                  ... on ProjectV2SingleSelectField {{
                    name
                  }}
                }}
              }}
              ... on ProjectV2ItemFieldTextValue {{
                text
                field {{
                  ... on ProjectV2FieldCommon {{
                    name
                  }}
                }}
              }}
              ... on ProjectV2ItemFieldNumberValue {{
                number
                field {{
                  ... on ProjectV2FieldCommon {{
                    name
                  }}
                }}
              }}
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    )
}

pub(in crate::tracker) fn github_project_metadata_query(owner_field: &str) -> String {
    format!(
        r#"
query SheaSymphonyProjectMetadata($owner: String!, $number: Int!) {{
  rateLimit {{
    cost
    remaining
    resetAt
  }}
  {owner_field}(login: $owner) {{
    projectV2(number: $number) {{
      id
      fields(first: {GITHUB_PROJECT_METADATA_FIELD_PAGE_SIZE}) {{
        nodes {{
          ... on ProjectV2FieldCommon {{
            id
            name
          }}
          __typename
          ... on ProjectV2SingleSelectField {{
            id
            name
            options {{
              id
              name
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    )
}

pub(in crate::tracker) const GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION: &str = r#"
mutation SheaSymphonyUpdateProjectStatus($projectId: ID!, $itemId: ID!, $fieldId: ID!, $optionId: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId,
    value: { singleSelectOptionId: $optionId }
  }) {
    projectV2Item {
      id
    }
  }
}
"#;

pub(in crate::tracker) const GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION: &str = r#"
mutation SheaSymphonyUpdateProjectTextField($projectId: ID!, $itemId: ID!, $fieldId: ID!, $text: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId,
    value: { text: $text }
  }) {
    projectV2Item {
      id
    }
  }
}
"#;

pub(in crate::tracker) const GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION: &str = r#"
mutation SheaSymphonyClearProjectField($projectId: ID!, $itemId: ID!, $fieldId: ID!) {
  clearProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId
  }) {
    projectV2Item {
      id
    }
  }
}
"#;

pub(in crate::tracker) fn github_issue_comments_query() -> String {
    format!(
        r#"
query SheaSymphonyIssueComments($issueId: ID!) {{
  rateLimit {{
    cost
    remaining
    resetAt
  }}
  node(id: $issueId) {{
    ... on Issue {{
      comments(first: {GITHUB_WORKPAD_COMMENT_PAGE_SIZE}) {{
        nodes {{
          id
          body
        }}
      }}
    }}
  }}
}}
"#
    )
}

pub(in crate::tracker) const GITHUB_UPDATE_ISSUE_COMMENT_MUTATION: &str = r#"
mutation SheaSymphonyUpdateIssueComment($commentId: ID!, $body: String!) {
  updateIssueComment(input: { id: $commentId, body: $body }) {
    issueComment {
      id
    }
  }
}
"#;

pub(in crate::tracker) const GITHUB_ADD_COMMENT_MUTATION: &str = r#"
mutation SheaSymphonyAddComment($subjectId: ID!, $body: String!) {
  addComment(input: { subjectId: $subjectId, body: $body }) {
    commentEdge {
      node {
        id
      }
    }
  }
}
"#;

pub(in crate::tracker) const GITHUB_CLOSE_ISSUE_MUTATION: &str = r#"
mutation SheaSymphonyCloseIssue($issueId: ID!) {
  closeIssue(input: { issueId: $issueId, stateReason: COMPLETED }) {
    issue {
      id
      state
    }
  }
}
"#;

pub(in crate::tracker) const GITHUB_REPOSITORY_ID_QUERY: &str = r#"
query SheaSymphonyRepositoryId($owner: String!, $repo: String!) {
  rateLimit {
    cost
    remaining
    resetAt
  }
  repository(owner: $owner, name: $repo) {
    id
  }
}
"#;

pub(in crate::tracker) const GITHUB_CREATE_ISSUE_MUTATION: &str = r#"
mutation SheaSymphonyCreateIssue($repositoryId: ID!, $title: String!, $body: String!) {
  createIssue(input: { repositoryId: $repositoryId, title: $title, body: $body }) {
    issue {
      id
      number
      url
    }
  }
}
"#;

pub(in crate::tracker) const GITHUB_ADD_PROJECT_ITEM_MUTATION: &str = r#"
mutation SheaSymphonyAddProjectItem($projectId: ID!, $contentId: ID!) {
  addProjectV2ItemById(input: { projectId: $projectId, contentId: $contentId }) {
    item {
      id
    }
  }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_read_queries_request_rate_limit_evidence() {
        let queries = [
            github_project_query("user", GithubProjectReadMode::QueueScan),
            github_issue_evidence_query(),
            github_issue_project_item_query(),
            github_project_metadata_query("user"),
            github_issue_comments_query(),
            GITHUB_REPOSITORY_ID_QUERY.to_string(),
        ];

        for query in queries {
            assert!(
                query.contains("rateLimit"),
                "query omitted rateLimit: {query}"
            );
            assert!(query.contains("cost"), "query omitted cost: {query}");
            assert!(
                query.contains("remaining"),
                "query omitted remaining: {query}"
            );
            assert!(query.contains("resetAt"), "query omitted resetAt: {query}");
        }
    }
}
