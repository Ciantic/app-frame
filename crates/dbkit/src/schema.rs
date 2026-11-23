//! Comparison of a crate's models against a live database.

use crate::TableMeta;
use sqlx::AssertSqlSafe;
use sqlx::PgConnection;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

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
pub async fn check_live_schema_against_models(
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
        let mut declared = BTreeSet::new();
        let mut duplicates = false;
        for column in meta.columns {
            if !declared.insert(column.name) {
                duplicates = true;
                problems.push(format!(
                    "{}: column \"{}\" is declared more than once",
                    meta.table, column.name
                ));
            }
        }

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

        if !duplicates {
            problems.extend(column_mismatches(connection, meta).await?);
        }
    }

    for table in tables {
        if !models.iter().any(|meta| meta.table == table) {
            problems.push(format!("table \"{table}\" has no model"));
        }
    }

    Ok(problems)
}

/// Lets PostgreSQL parse the `pgtype` and `pgdefault` annotations and compares
/// the resulting type, typmod and default with the columns of the real table.
async fn column_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
) -> sqlx::Result<Vec<String>> {
    let mut problems = Vec::new();
    let probe = format!("pgtype_probe_{}", meta.table);
    let definition: Vec<String> = meta
        .columns
        .iter()
        .map(|column| match column.pg_default {
            Some(default) => format!("\"{}\" {} DEFAULT {default}", column.name, column.pg_type),
            None => format!("\"{}\" {}", column.name, column.pg_type),
        })
        .collect();
    let type_definition: Vec<String> = meta
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
        // Types-only retry attributes the failure to the annotation that caused it.
        let types_only = format!(
            "CREATE TEMPORARY TABLE pg_temp.\"{probe}\" ({})",
            type_definition.join(", ")
        );
        let type_error = sqlx::raw_sql(AssertSqlSafe(types_only))
            .execute(&mut *connection)
            .await
            .err();

        return Ok(vec![if type_error.is_some() {
            format!(
                "{}: `pgtype` is not valid in a column definition: {error}",
                meta.table
            )
        } else {
            format!(
                "{}: `pgdefault` is not valid in a column definition: {error}",
                meta.table
            )
        }]);
    }

    let columns: Vec<ProbeColumn> = sqlx::query_as(
        "SELECT probe.attname AS name,
                format_type(probe.atttypid, probe.atttypmod) AS probe_type,
                format_type(real.atttypid, real.atttypmod) AS real_type,
                pg_get_expr(probe_default.adbin, probe_default.adrelid) AS probe_default,
                pg_get_expr(real_default.adbin, real_default.adrelid) AS real_default
         FROM pg_attribute probe
         LEFT JOIN pg_attribute real
           ON real.attrelid = to_regclass($2) AND real.attname = probe.attname
         LEFT JOIN pg_attrdef probe_default
           ON probe_default.adrelid = probe.attrelid
          AND probe_default.adnum = probe.attnum
         LEFT JOIN pg_attrdef real_default
           ON real_default.adrelid = real.attrelid
          AND real_default.adnum = real.attnum
         WHERE probe.attrelid = to_regclass($1)
           AND probe.attnum > 0 AND NOT probe.attisdropped",
    )
    .bind(format!("pg_temp.\"{probe}\""))
    .bind(format!("\"{}\"", meta.table))
    .fetch_all(&mut *connection)
    .await?;

    for column in meta.columns {
        let Some(probe_column) = columns.iter().find(|probe| probe.name == column.name) else {
            continue;
        };
        let Some(real_type) = &probe_column.real_type else {
            continue;
        };

        if &probe_column.probe_type != real_type {
            problems.push(format!(
                "{}: column \"{}\" is {real_type}, the model says {}",
                meta.table, column.name, column.pg_type
            ));
        }

        if probe_column.probe_default != probe_column.real_default {
            problems.push(format!(
                "{}: column \"{}\" default is {}, the model says {}",
                meta.table,
                column.name,
                probe_column.real_default.as_deref().unwrap_or("none"),
                probe_column.probe_default.as_deref().unwrap_or("none"),
            ));
        }
    }

    Ok(problems)
}

#[derive(sqlx::FromRow)]
struct ProbeColumn {
    name: String,
    probe_type: String,
    real_type: Option<String>,
    probe_default: Option<String>,
    real_default: Option<String>,
}
