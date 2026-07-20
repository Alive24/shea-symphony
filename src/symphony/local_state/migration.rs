use rusqlite::{params_from_iter, Transaction};
use sea_query::{
    ColumnDef, ConditionalStatement, Expr, ExprTrait, Index, Query, SchemaStatementBuilder,
    SqliteQueryBuilder, Table,
};
use sea_query_rusqlite::RusqliteBinder;

pub(super) const CURRENT_SCHEMA_VERSION: u32 = 1;

pub(super) struct Migration {
    pub version: u32,
    pub apply: fn(&Transaction<'_>, &str) -> rusqlite::Result<()>,
}

pub(super) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    apply: apply_v1,
}];

fn execute_schema(
    transaction: &Transaction<'_>,
    statement: impl SchemaStatementBuilder,
) -> rusqlite::Result<()> {
    transaction.execute(&statement.to_string(SqliteQueryBuilder), [])?;
    Ok(())
}

fn apply_v1(transaction: &Transaction<'_>, created_at: &str) -> rusqlite::Result<()> {
    // SeaQuery is the single executable v1 schema authority. There are no
    // IF NOT EXISTS clauses because version-zero drift must remain observable.
    execute_schema(
        transaction,
        Table::create()
            .table("workflow_index")
            .col(
                ColumnDef::new("workflow_id")
                    .text()
                    .not_null()
                    .primary_key(),
            )
            .col(ColumnDef::new("run_id").text())
            .col(ColumnDef::new("workspace_runtime_id").text().not_null())
            .col(ColumnDef::new("repo_id").text().not_null())
            .col(ColumnDef::new("issue_ref").text().not_null())
            .col(ColumnDef::new("from_state").text().not_null())
            .col(ColumnDef::new("target_kind").text().not_null())
            .col(ColumnDef::new("current_state").text().not_null())
            .col(ColumnDef::new("active_step").text().not_null())
            .col(ColumnDef::new("waiting_kind").text())
            .col(ColumnDef::new("source_ref").text().not_null())
            .col(ColumnDef::new("source_tracker_revision").text().not_null())
            .col(ColumnDef::new("started_at").text().not_null())
            .col(ColumnDef::new("last_progress_at").text())
            .col(ColumnDef::new("status").text().not_null())
            .col(ColumnDef::new("terminal_outcome").text())
            .col(ColumnDef::new("operator_action_ref").text())
            .col(ColumnDef::new("freshness").text().not_null())
            .col(ColumnDef::new("updated_at").text().not_null())
            .to_owned(),
    )?;
    execute_schema(
        transaction,
        Table::create()
            .table("artifact_index")
            .col(
                ColumnDef::new("artifact_id")
                    .text()
                    .not_null()
                    .primary_key(),
            )
            .col(ColumnDef::new("workflow_id").text())
            .col(ColumnDef::new("workspace_runtime_id").text().not_null())
            .col(ColumnDef::new("repo_id").text().not_null())
            .col(ColumnDef::new("issue_ref").text().not_null())
            .col(ColumnDef::new("kind").text().not_null())
            .col(ColumnDef::new("path").text().not_null())
            .col(ColumnDef::new("summary").text())
            .col(ColumnDef::new("created_by_step").text())
            .col(ColumnDef::new("created_at").text().not_null())
            .to_owned(),
    )?;
    execute_schema(
        transaction,
        Table::create()
            .table("tracker_cache")
            .col(ColumnDef::new("workspace_runtime_id").text().not_null())
            .col(ColumnDef::new("repo_id").text().not_null())
            .col(ColumnDef::new("issue_ref").text().not_null())
            .col(ColumnDef::new("tracker_backend").text().not_null())
            .col(ColumnDef::new("tracker_state").text().not_null())
            .col(ColumnDef::new("title").text().not_null())
            .col(ColumnDef::new("pr_number").integer())
            .col(ColumnDef::new("pr_state").text())
            .col(ColumnDef::new("pr_relation_confirmed_at").text())
            .col(ColumnDef::new("updated_at").text().not_null())
            .col(ColumnDef::new("freshness").text().not_null())
            .primary_key(
                Index::create()
                    .name("pk_tracker_cache")
                    .col("workspace_runtime_id")
                    .col("repo_id")
                    .col("issue_ref"),
            )
            .to_owned(),
    )?;
    execute_schema(
        transaction,
        Table::create()
            .table("activity_progress")
            .col(ColumnDef::new("workspace_runtime_id").text().not_null())
            .col(ColumnDef::new("workflow_id").text().not_null())
            .col(ColumnDef::new("activity_id").text().not_null())
            .col(ColumnDef::new("activity_kind").text().not_null())
            .col(ColumnDef::new("target_ref").text().not_null())
            .col(ColumnDef::new("mutation_id").text())
            .col(ColumnDef::new("outcome").text())
            .col(ColumnDef::new("status").text().not_null())
            .col(ColumnDef::new("attempt_count").integer().not_null())
            .col(ColumnDef::new("last_heartbeat_at").text())
            .col(ColumnDef::new("next_retry_at").text())
            .col(ColumnDef::new("summary").text())
            .primary_key(
                Index::create()
                    .name("pk_activity_progress")
                    .col("workflow_id")
                    .col("activity_id"),
            )
            .to_owned(),
    )?;
    execute_schema(
        transaction,
        Table::create()
            .table("meta")
            .col(ColumnDef::new("key").text().not_null().primary_key())
            .col(ColumnDef::new("value").text().not_null())
            .to_owned(),
    )?;

    for index in [
        Index::create()
            .name("idx_workflow_index_scope_issue_status")
            .table("workflow_index")
            .col("workspace_runtime_id")
            .col("repo_id")
            .col("issue_ref")
            .col("status")
            .to_owned(),
        Index::create()
            .name("idx_workflow_index_scope_lane")
            .table("workflow_index")
            .col("workspace_runtime_id")
            .col("current_state")
            .col("waiting_kind")
            .to_owned(),
        Index::create()
            .name("idx_artifact_index_scope_workflow")
            .table("artifact_index")
            .col("workspace_runtime_id")
            .col("workflow_id")
            .to_owned(),
        Index::create()
            .name("idx_artifact_index_scope_issue")
            .table("artifact_index")
            .col("workspace_runtime_id")
            .col("repo_id")
            .col("issue_ref")
            .to_owned(),
        Index::create()
            .name("idx_tracker_cache_scope_freshness")
            .table("tracker_cache")
            .col("workspace_runtime_id")
            .col("freshness")
            .to_owned(),
        Index::create()
            .name("idx_activity_progress_scope_workflow_mutation")
            .table("activity_progress")
            .col("workspace_runtime_id")
            .col("workflow_id")
            .col("mutation_id")
            .to_owned(),
    ] {
        execute_schema(transaction, index)?;
    }

    execute_schema(
        transaction,
        Index::create()
            .name("uq_workflow_index_active_issue")
            .table("workflow_index")
            .col("repo_id")
            .col("issue_ref")
            .unique()
            .and_where(Expr::col("status").is_in(["starting", "running"]))
            .to_owned(),
    )?;

    let (sql, values) = Query::insert()
        .into_table("meta")
        .columns(["key", "value"])
        .values_panic(["created_at".into(), created_at.into()])
        .values_panic(["updated_at".into(), created_at.into()])
        .build_rusqlite(SqliteQueryBuilder);
    transaction.execute(&sql, params_from_iter(values.as_params()))?;
    Ok(())
}
