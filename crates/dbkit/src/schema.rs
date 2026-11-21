//! Comparison of a crate's models against a live database.

use crate::TableMeta;
use sqlx::AssertSqlSafe;
use sqlx::PgConnection;
use std::collections::BTreeMap;

/// Compares `models` with the tables of the connection's current schema.
///
/// Returns one message per difference: a column or table without a model, a
/// model field without a column, and a nullable or type mismatch. An empty list
/// means they agree.
///
/// Types are compared by creating a temporary table from the `pgtype`
/// annotations and reading back the type and typmod PostgreSQL gave each
/// column, so any DDL spelling is accepted and length and precision must match.
/// The temporary table is session-local, hence the connection rather than a
/// pool.
pub async fn mismatches(
    connection: &mut PgConnection,
    models: &[TableMeta],
) -> sqlx::Result<Vec<String>> {
    let mut problems = Vec::new();

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
    )
    .fetch_all(&mut *connection)
    .await?;

    for meta in models {
        let schema_columns: Vec<(String, String)> = sqlx::query_as(
            "SELECT column_name, is_nullable FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = $1",
        )
        .bind(meta.table)
        .fetch_all(&mut *connection)
        .await?;
        let schema_columns: BTreeMap<String, bool> = schema_columns
            .into_iter()
            .map(|(name, nullable)| (name, nullable == "YES"))
            .collect();

        for (column, nullable) in &schema_columns {
            match meta.columns.iter().find(|model| model.name == column) {
                None => problems.push(format!(
                    "table \"{}\" has column \"{column}\" without a model field",
                    meta.table
                )),
                Some(model) if model.nullable != *nullable => problems.push(format!(
                    "{}: column \"{column}\" is nullable = {nullable}, the model says {}",
                    meta.table, model.nullable
                )),
                Some(_) => {}
            }
        }
        for column in meta.columns {
            if !schema_columns.contains_key(column.name) {
                problems.push(format!(
                    "{}: field \"{}\" has no column",
                    meta.table, column.name
                ));
            }
        }

        problems.extend(type_mismatches(connection, meta).await?);
    }

    for table in tables {
        if !models.iter().any(|meta| meta.table == table) {
            problems.push(format!("table \"{table}\" has no model"));
        }
    }

    Ok(problems)
}

/// Lets PostgreSQL parse the `pgtype` annotations and compares the resulting
/// type and typmod with the columns of the real table.
async fn type_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
) -> sqlx::Result<Vec<String>> {
    let mut problems = Vec::new();
    let probe = format!("pgtype_probe_{}", meta.table);
    let definition: Vec<String> = meta
        .columns
        .iter()
        .map(|column| format!("\"{}\" {}", column.name, column.pg_type))
        .collect();

    let drop = format!("DROP TABLE IF EXISTS pg_temp.\"{probe}\"");
    sqlx::raw_sql(AssertSqlSafe(drop))
        .execute(&mut *connection)
        .await?;

    let create = format!(
        "CREATE TEMPORARY TABLE pg_temp.\"{probe}\" ({})",
        definition.join(", ")
    );
    if let Err(error) = sqlx::raw_sql(AssertSqlSafe(create))
        .execute(&mut *connection)
        .await
    {
        return Ok(vec![format!(
            "{}: `pgtype` is not a valid column type: {error}",
            meta.table
        )]);
    }

    let types: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT probe.attname,
                format_type(probe.atttypid, probe.atttypmod),
                format_type(real.atttypid, real.atttypmod)
         FROM pg_attribute probe
         LEFT JOIN pg_attribute real
           ON real.attrelid = to_regclass($2) AND real.attname = probe.attname
         WHERE probe.attrelid = to_regclass($1)
           AND probe.attnum > 0 AND NOT probe.attisdropped",
    )
    .bind(format!("pg_temp.\"{probe}\""))
    .bind(format!("\"{}\"", meta.table))
    .fetch_all(&mut *connection)
    .await?;
    let types: BTreeMap<String, (String, Option<String>)> = types
        .into_iter()
        .map(|(name, probe_type, real_type)| (name, (probe_type, real_type)))
        .collect();

    for column in meta.columns {
        let Some((probe_type, Some(real_type))) = types.get(column.name) else {
            continue;
        };

        if probe_type != real_type {
            problems.push(format!(
                "{}: column \"{}\" is {real_type}, the model says {}",
                meta.table, column.name, column.pg_type
            ));
        }
    }

    Ok(problems)
}
