//! Comparison of a crate's models against a live database.

use crate::TableMeta;
use sqlx::AssertSqlSafe;
use sqlx::PgConnection;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Compares `models` with the connection's tables; one message per difference, and an empty list means they agree.
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
            if !declared.insert(column.name()) {
                duplicates = true;
                problems.push(format!(
                    "{}: column \"{}\" is declared more than once",
                    meta.table,
                    column.name()
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
            match meta.columns.iter().find(|model| model.name() == column) {
                None => problems.push(format!(
                    "table \"{}\" has column \"{column}\" without a model field",
                    meta.table
                )),
                Some(model) if model.nullable() != *nullable => problems.push(format!(
                    "{}: column \"{column}\" is nullable = {nullable}, the model says {}",
                    meta.table,
                    model.nullable()
                )),
                Some(_) => {}
            }
        }
        for column in meta.columns {
            if !schema_columns.contains_key(column.name()) {
                problems.push(format!(
                    "{}: field \"{}\" has no column",
                    meta.table,
                    column.name()
                ));
            }
        }

        if !duplicates {
            problems.extend(column_mismatches(connection, meta).await?);
        }
        problems.extend(foreign_key_mismatches(connection, meta).await?);
        problems.extend(trigger_mismatches(connection, meta).await?);
        problems.extend(unique_key_mismatches(connection, meta).await?);
    }

    for table in tables {
        if !models.iter().any(|meta| meta.table == table) {
            problems.push(format!("table \"{table}\" has no model"));
        }
    }

    Ok(problems)
}

/// Lets PostgreSQL parse the `pgtype` and `pgdefault` DDL and compares the result with the real columns.
async fn column_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
) -> sqlx::Result<Vec<String>> {
    let mut problems = Vec::new();
    let probe = format!("pgtype_probe_{}", meta.table);
    let definition: Vec<String> = meta
        .columns
        .iter()
        .map(|column| match column.generated() {
            Some(expression) => format!(
                "\"{}\" {} GENERATED ALWAYS AS ({expression}) VIRTUAL",
                column.name(),
                column.pg_type()
            ),
            None => match column.pg_default() {
                Some(default) => format!(
                    "\"{}\" {} DEFAULT {default}",
                    column.name(),
                    column.pg_type()
                ),
                None => format!("\"{}\" {}", column.name(), column.pg_type()),
            },
        })
        .collect();
    let generated = meta
        .columns
        .iter()
        .any(|column| column.generated().is_some());
    let type_definition: Vec<String> = meta
        .columns
        .iter()
        .map(|column| format!("\"{}\" {}", column.name(), column.pg_type()))
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
            let annotation = if generated {
                "`pgdefault` or `pg_virtual`"
            } else {
                "`pgdefault`"
            };
            format!(
                "{}: {annotation} is not valid in a column definition: {error}",
                meta.table
            )
        }]);
    }

    let columns: Vec<ProbeColumn> = sqlx::query_as(
        "SELECT probe.attname AS name,
                format_type(probe.atttypid, probe.atttypmod) AS probe_type,
                format_type(real.atttypid, real.atttypmod) AS real_type,
                probe.attgenerated::text AS probe_generated,
                COALESCE(real.attgenerated::text, '') AS real_generated,
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
        let Some(probe_column) = columns.iter().find(|probe| probe.name == column.name()) else {
            continue;
        };
        let Some(real_type) = &probe_column.real_type else {
            continue;
        };

        if &probe_column.probe_type != real_type {
            problems.push(format!(
                "{}: column \"{}\" is {real_type}, the model says {}",
                meta.table,
                column.name(),
                column.pg_type()
            ));
        }

        if probe_column.probe_generated != probe_column.real_generated {
            problems.push(format!(
                "{}: column \"{}\" generated is {}, the model says {}",
                meta.table,
                column.name(),
                generated_kind(&probe_column.real_generated),
                generated_kind(&probe_column.probe_generated),
            ));
            continue;
        }

        if probe_column.probe_default != probe_column.real_default {
            let label = if probe_column.probe_generated.is_empty() {
                "default"
            } else {
                "generated expression"
            };
            problems.push(format!(
                "{}: column \"{}\" {label} is {}, the model says {}",
                meta.table,
                column.name(),
                probe_column.real_default.as_deref().unwrap_or("none"),
                probe_column.probe_default.as_deref().unwrap_or("none"),
            ));
        }
    }

    Ok(problems)
}

/// Compares the model's foreign keys with the live constraints on its table.
async fn foreign_key_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
) -> sqlx::Result<Vec<String>> {
    let live: Vec<LiveForeignKey> = sqlx::query_as(
        "SELECT con.conname AS name,
                target.relname AS target,
                ARRAY(
                    SELECT att.attname
                    FROM unnest(con.conkey::int2[]) WITH ORDINALITY AS k(attnum, ord)
                    JOIN pg_attribute att
                      ON att.attrelid = con.conrelid
                     AND att.attnum = k.attnum
                    ORDER BY k.ord
                ) AS columns,
                ARRAY(
                    SELECT att.attname
                    FROM unnest(con.confkey::int2[]) WITH ORDINALITY AS k(attnum, ord)
                    JOIN pg_attribute att
                      ON att.attrelid = con.confrelid
                     AND att.attnum = k.attnum
                    ORDER BY k.ord
                ) AS referenced_columns
         FROM pg_constraint con
         JOIN pg_class target ON target.oid = con.confrelid
         WHERE con.contype = 'f'
           AND con.conrelid = to_regclass($1)",
    )
    .bind(quoted_table(meta.table))
    .fetch_all(&mut *connection)
    .await?;

    let mut problems = Vec::new();
    for declared in meta.foreign_keys {
        match live
            .iter()
            .find(|candidate| same_columns(&candidate.columns, declared.columns))
        {
            None => problems.push(format!(
                "{}: foreign key {} is declared but no constraint exists",
                meta.table,
                column_list(declared.columns),
            )),
            Some(live)
                if live.target != declared.table
                    || !same_columns(&live.referenced_columns, declared.referenced_columns) =>
            {
                problems.push(format!(
                    "{}: foreign key {} references {}{}, the model says {}{}",
                    meta.table,
                    column_list(declared.columns),
                    live.target,
                    column_list(&live.referenced_columns),
                    declared.table,
                    column_list(declared.referenced_columns),
                ));
            }
            Some(_) => {}
        }
    }

    for live in &live {
        if !meta
            .foreign_keys
            .iter()
            .any(|declared| same_columns(&live.columns, declared.columns))
        {
            problems.push(format!(
                "{}: foreign key constraint `{}` on {} has no model declaration",
                meta.table,
                live.name,
                column_list(&live.columns),
            ));
        }
    }

    Ok(problems)
}

/// Compares the model's trigger names with the table's non-internal triggers.
async fn trigger_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
) -> sqlx::Result<Vec<String>> {
    let live: Vec<String> = sqlx::query_scalar(
        "SELECT t.tgname
         FROM pg_trigger t
         WHERE t.tgrelid = to_regclass($1)
           AND NOT t.tgisinternal
         ORDER BY t.tgname",
    )
    .bind(quoted_table(meta.table))
    .fetch_all(&mut *connection)
    .await?;

    let declared: BTreeSet<&str> = meta.triggers.iter().copied().collect();
    let live: BTreeSet<&str> = live.iter().map(String::as_str).collect();
    let mut problems = Vec::new();

    for trigger in declared.difference(&live) {
        problems.push(format!(
            "{}: trigger `{trigger}` is declared but does not exist",
            meta.table
        ));
    }
    for trigger in live.difference(&declared) {
        problems.push(format!(
            "{}: trigger `{trigger}` has no model declaration",
            meta.table
        ));
    }

    Ok(problems)
}

/// Compares the model's unique keys with the table's unique, non-primary indexes.
async fn unique_key_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
) -> sqlx::Result<Vec<String>> {
    let live: Vec<LiveUnique> = sqlx::query_as(
        "SELECT idx.indexrelid::regclass::text AS name,
                ARRAY(
                    SELECT att.attname
                    FROM unnest(idx.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord)
                    JOIN pg_attribute att
                      ON att.attrelid = idx.indrelid
                     AND att.attnum = k.attnum
                    WHERE k.ord <= idx.indnkeyatts
                    ORDER BY k.ord
                ) AS columns
         FROM pg_index idx
         WHERE idx.indrelid = to_regclass($1)
           AND idx.indisunique
           AND NOT idx.indisprimary
           AND idx.indpred IS NULL
           AND idx.indexprs IS NULL",
    )
    .bind(quoted_table(meta.table))
    .fetch_all(&mut *connection)
    .await?;

    let declared: BTreeSet<Vec<String>> = meta
        .unique_keys
        .iter()
        .map(|key| normalized_columns(key.columns))
        .collect();
    let live_columns: BTreeSet<Vec<String>> = live
        .iter()
        .map(|index| normalized_columns(&index.columns))
        .collect();
    let mut problems = Vec::new();

    for key in declared.difference(&live_columns) {
        problems.push(format!(
            "{}: unique key {} is declared but no index exists",
            meta.table,
            column_list(key),
        ));
    }
    for index in live
        .iter()
        .filter(|index| !declared.contains(&normalized_columns(&index.columns)))
    {
        problems.push(format!(
            "{}: unique index `{}` on {} has no model declaration",
            meta.table,
            index.name,
            column_list(&index.columns),
        ));
    }

    Ok(problems)
}

fn quoted_table(table: &str) -> String {
    format!("\"{table}\"")
}

fn same_columns(live: &[String], declared: &[&str]) -> bool {
    live.len() == declared.len()
        && live
            .iter()
            .zip(declared)
            .all(|(live, declared)| live == declared)
}

fn normalized_columns(columns: &[impl AsRef<str>]) -> Vec<String> {
    let mut columns: Vec<String> = columns
        .iter()
        .map(|column| column.as_ref().to_owned())
        .collect();
    columns.sort();
    columns
}

fn column_list(columns: &[impl AsRef<str>]) -> String {
    format!(
        "({})",
        columns
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[derive(sqlx::FromRow)]
struct LiveForeignKey {
    name: String,
    target: String,
    columns: Vec<String>,
    referenced_columns: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct LiveUnique {
    name: String,
    columns: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct ProbeColumn {
    name: String,
    probe_type: String,
    real_type: Option<String>,
    probe_generated: String,
    real_generated: String,
    probe_default: Option<String>,
    real_default: Option<String>,
}

fn generated_kind(kind: &str) -> &'static str {
    match kind {
        "s" => "stored",
        "v" => "virtual",
        _ => "none",
    }
}
