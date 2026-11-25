//! Service schema composition and comparison of a crate's models against a live database.

use crate::TableMeta;
use crate::Trigger;
use crate::TriggerEvent;
use crate::TriggerLevel;
use crate::TriggerTiming;
use sqlx::AssertSqlSafe;
use sqlx::PgConnection;
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// One service's schema fragment, its models and the fragments it depends on.
pub struct Schema {
    pub name: &'static str,
    pub sql: &'static str,
    pub models: &'static LazyLock<Vec<TableMeta>>,
    pub dependencies: &'static [&'static Schema],
}

impl Schema {
    /// Applies every dependency fragment and this fragment, dependencies first.
    pub async fn apply(&self, pool: &PgPool) -> sqlx::Result<()> {
        sqlx::raw_sql(AssertSqlSafe(self.all_sql()))
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Checks the live schema against this fragment and all of its dependencies.
    pub async fn check(&self, connection: &mut PgConnection) -> sqlx::Result<Vec<String>> {
        check_live_schema_against_models(connection, &self.all_models()).await
    }

    /// The dependencies' SQL followed by this fragment's, each service once.
    pub fn all_sql(&self) -> String {
        let mut visited = BTreeSet::new();
        let mut statements = Vec::new();
        self.collect_sql(&mut visited, &mut statements);
        statements.join("\n")
    }

    /// The dependencies' models followed by this fragment's, each service once.
    pub fn all_models(&self) -> Vec<TableMeta> {
        let mut visited = BTreeSet::new();
        let mut models = Vec::new();
        self.collect_models(&mut visited, &mut models);
        models
    }

    fn collect_sql(
        &self,
        visited: &mut BTreeSet<&'static str>,
        statements: &mut Vec<&'static str>,
    ) {
        if !visited.insert(self.name) {
            return;
        }
        for dependency in self.dependencies {
            dependency.collect_sql(visited, statements);
        }
        statements.push(self.sql);
    }

    fn collect_models(&self, visited: &mut BTreeSet<&'static str>, models: &mut Vec<TableMeta>) {
        if !visited.insert(self.name) {
            return;
        }
        for dependency in self.dependencies {
            dependency.collect_models(visited, models);
        }
        models.extend(self.models.iter().copied());
    }
}

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
        problems.extend(unique_key_mismatches(connection, meta).await?);
    }
    problems.extend(trigger_mismatches(connection, models).await?);

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

/// Compares every model's trigger declarations with the live triggers, including definitions on another table.
async fn trigger_mismatches(
    connection: &mut PgConnection,
    models: &[TableMeta],
) -> sqlx::Result<Vec<String>> {
    let mut problems = Vec::new();
    let mut declared: BTreeMap<&str, BTreeMap<&str, Option<&Trigger>>> = BTreeMap::new();
    let mut definitions: BTreeMap<(&str, &str), &Trigger> = BTreeMap::new();

    for meta in models {
        for trigger in meta.triggers {
            declared
                .entry(trigger.table)
                .or_default()
                .entry(trigger.name)
                .or_insert(None);
        }
        for definition in meta.trigger_definitions {
            let key = (definition.table, definition.name);
            match definitions.get(&key) {
                Some(existing) if *existing != definition => problems.push(format!(
                    "trigger `{}` on \"{}\" has conflicting definitions in the models",
                    definition.name, definition.table
                )),
                Some(_) => {}
                None => {
                    definitions.insert(key, definition);
                }
            }
        }
    }

    for ((table, name), definition) in &definitions {
        if let Some(table_triggers) = declared.get_mut(table)
            && let Some(slot) = table_triggers.get_mut(name)
        {
            *slot = Some(definition);
        }
    }

    let mut checked = BTreeSet::new();
    for meta in models {
        let table_triggers = declared.get(meta.table);
        let live = live_triggers(connection, meta.table).await?;

        if let Some(table_triggers) = table_triggers {
            for (&name, definition) in table_triggers {
                let Some(live) = live.iter().find(|live| live.name == name) else {
                    problems.push(format!(
                        "{}: trigger `{name}` is declared but does not exist",
                        meta.table
                    ));
                    continue;
                };
                if let Some(definition) = definition {
                    problems.extend(
                        trigger_definition_mismatches(connection, meta, live, definition).await?,
                    );
                }
            }
        }

        for live in &live {
            if !table_triggers.is_some_and(|triggers| triggers.contains_key(live.name.as_str())) {
                problems.push(format!(
                    "{}: trigger `{}` has no model declaration",
                    meta.table, live.name
                ));
            }
        }

        checked.insert(meta.table);
    }

    for (table, table_triggers) in &declared {
        if checked.contains(table) {
            continue;
        }
        for name in table_triggers.keys() {
            problems.push(format!(
                "trigger `{name}` is declared on table \"{table}\", which has no model"
            ));
        }
    }

    Ok(problems)
}

/// Compares one live trigger's timing, events, level, condition, function and body with its definition.
async fn trigger_definition_mismatches(
    connection: &mut PgConnection,
    meta: &TableMeta,
    live: &LiveTrigger,
    declared: &Trigger,
) -> sqlx::Result<Vec<String>> {
    let table = meta.table;
    let mut problems = Vec::new();
    let timing = live.timing();
    if timing != declared.timing {
        problems.push(format!(
            "{table}: trigger `{}` timing is {}, the model says {}",
            declared.name,
            timing.as_str(),
            declared.timing.as_str()
        ));
    }
    let level = live.level();
    if level != declared.level {
        problems.push(format!(
            "{table}: trigger `{}` level is {}, the model says {}",
            declared.name,
            level.as_str(),
            declared.level.as_str()
        ));
    }
    let events = live.events();
    if events.as_slice() != declared.events {
        problems.push(format!(
            "{table}: trigger `{}` events are {}, the model says {}",
            declared.name,
            trigger_events(&events),
            trigger_events(declared.events)
        ));
    }

    match (declared.condition, live.condition.as_deref()) {
        (None, None) => {}
        (None, Some(_)) => problems.push(format!(
            "{table}: trigger `{}` condition is {}, the model says none",
            declared.name,
            trigger_condition(&live.definition).unwrap_or("present")
        )),
        (Some(condition), None) => problems.push(format!(
            "{table}: trigger `{}` condition is none, the model says {condition}",
            declared.name
        )),
        (Some(condition), Some(_)) => {
            match probe_trigger_condition(connection, meta, declared, condition).await {
                Ok(probed) if probed.as_deref() == live.condition.as_deref() => {}
                Ok(_) => problems.push(format!(
                    "{table}: trigger `{}` condition is {}, the model says {condition}",
                    declared.name,
                    trigger_condition(&live.definition).unwrap_or("present")
                )),
                Err(error) => problems.push(format!(
                    "{table}: trigger `{}` condition is not valid in a trigger definition: {error}",
                    declared.name
                )),
            }
        }
    }

    if live.function != declared.function {
        problems.push(format!(
            "{table}: trigger `{}` function is `{}`, the model says `{}`",
            declared.name, live.function, declared.function
        ));
    }
    if live.language != declared.language {
        problems.push(format!(
            "{table}: trigger `{}` language is `{}`, the model says `{}`",
            declared.name, live.language, declared.language
        ));
    }
    if live.body.trim() != declared.body.trim() {
        problems.push(format!(
            "{table}: trigger `{}` body differs from the model's definition",
            declared.name
        ));
    }

    Ok(problems)
}

/// Creates a temporary trigger with the declared condition so PostgreSQL parses both into comparable trees.
async fn probe_trigger_condition(
    connection: &mut PgConnection,
    meta: &TableMeta,
    declared: &Trigger,
    condition: &str,
) -> Result<Option<String>, String> {
    let probe = format!("{}_trigger_probe", meta.table);
    let function = format!("{probe}_fn");
    let probe_table = format!("pg_temp.\"{probe}\"");
    let drop_table = format!("DROP TABLE IF EXISTS {probe_table}");
    let drop_function = format!("DROP FUNCTION IF EXISTS pg_temp.\"{function}\"()");
    let _ = sqlx::raw_sql(AssertSqlSafe(drop_table.clone()))
        .execute(&mut *connection)
        .await;
    let _ = sqlx::raw_sql(AssertSqlSafe(drop_function.clone()))
        .execute(&mut *connection)
        .await;

    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT att.attname, format_type(att.atttypid, att.atttypmod)
         FROM pg_attribute att
         WHERE att.attrelid = to_regclass($1)
           AND att.attnum > 0 AND NOT att.attisdropped
         ORDER BY att.attnum",
    )
    .bind(quoted_table(meta.table))
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| error.to_string())?;
    let definition: Vec<String> = columns
        .iter()
        .map(|(name, pg_type)| format!("\"{name}\" {pg_type}"))
        .collect();
    let create_table = format!(
        "CREATE TEMPORARY TABLE {probe_table} ({})",
        definition.join(", ")
    );
    sqlx::raw_sql(AssertSqlSafe(create_table))
        .execute(&mut *connection)
        .await
        .map_err(|error| error.to_string())?;

    let create_function = format!(
        "CREATE FUNCTION pg_temp.\"{function}\"() RETURNS trigger LANGUAGE plpgsql AS $$ begin \
         return null; end; $$"
    );
    sqlx::raw_sql(AssertSqlSafe(create_function))
        .execute(&mut *connection)
        .await
        .map_err(|error| error.to_string())?;

    let timing = declared.timing.as_str();
    let events: Vec<&str> = declared.events.iter().map(|event| event.as_str()).collect();
    let create_trigger = format!(
        "CREATE TRIGGER \"{probe}\" {timing} {} ON {probe_table} FOR EACH ROW WHEN ({condition}) \
         EXECUTE FUNCTION pg_temp.\"{function}\"()",
        events.join(" OR "),
    );
    let created = sqlx::raw_sql(AssertSqlSafe(create_trigger))
        .execute(&mut *connection)
        .await;

    let probed = match created {
        Ok(_) => {
            let probed: Option<String> = sqlx::query_scalar(
                "SELECT tgqual::text FROM pg_trigger
                 WHERE tgrelid = to_regclass($1) AND tgname = $2",
            )
            .bind(&probe_table)
            .bind(&probe)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| error.to_string())?;

            Ok(probed)
        }
        Err(error) => Err(error.to_string()),
    };

    let _ = sqlx::raw_sql(AssertSqlSafe(drop_table))
        .execute(&mut *connection)
        .await;
    let _ = sqlx::raw_sql(AssertSqlSafe(drop_function))
        .execute(&mut *connection)
        .await;

    probed
}

/// The `WHEN` expression from `pg_get_triggerdef`, for a human-readable mismatch message.
fn trigger_condition(definition: &str) -> Option<&str> {
    let start = definition.find(" WHEN (")? + " WHEN (".len();
    let rest = &definition[start..];
    let end = rest.find(") EXECUTE ")?;

    Some(rest[..end].trim())
}

async fn live_triggers(
    connection: &mut PgConnection,
    table: &str,
) -> sqlx::Result<Vec<LiveTrigger>> {
    sqlx::query_as(
        "SELECT t.tgname AS name,
                t.tgtype::int AS kind,
                t.tgqual::text AS condition,
                pg_get_triggerdef(t.oid) AS definition,
                p.proname AS function,
                l.lanname AS language,
                p.prosrc AS body
         FROM pg_trigger t
         JOIN pg_proc p ON p.oid = t.tgfoid
         JOIN pg_language l ON l.oid = p.prolang
         WHERE t.tgrelid = to_regclass($1)
           AND NOT t.tgisinternal
         ORDER BY t.tgname",
    )
    .bind(quoted_table(table))
    .fetch_all(&mut *connection)
    .await
}

fn trigger_events(events: &[TriggerEvent]) -> String {
    format!(
        "({})",
        events
            .iter()
            .map(|event| event.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
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
struct LiveTrigger {
    name: String,
    kind: i32,
    condition: Option<String>,
    definition: String,
    function: String,
    language: String,
    body: String,
}

impl LiveTrigger {
    fn timing(&self) -> TriggerTiming {
        if self.kind & 2 != 0 {
            TriggerTiming::Before
        } else {
            TriggerTiming::After
        }
    }

    fn level(&self) -> TriggerLevel {
        if self.kind & 1 != 0 {
            TriggerLevel::Row
        } else {
            TriggerLevel::Statement
        }
    }

    fn events(&self) -> Vec<TriggerEvent> {
        TriggerEvent::ALL
            .iter()
            .copied()
            .filter(|event| {
                let bit = match event {
                    TriggerEvent::Insert => 4,
                    TriggerEvent::Update => 16,
                    TriggerEvent::Delete => 8,
                    TriggerEvent::Truncate => 32,
                };
                self.kind & bit != 0
            })
            .collect()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    static MODELS_A: LazyLock<Vec<TableMeta>> = LazyLock::new(Vec::new);
    static MODELS_B: LazyLock<Vec<TableMeta>> = LazyLock::new(Vec::new);
    static BASE: Schema = Schema {
        name: "base",
        sql: "base;",
        models: &MODELS_A,
        dependencies: &[],
    };
    static MIDDLE: Schema = Schema {
        name: "middle",
        sql: "middle;",
        models: &MODELS_B,
        dependencies: &[&BASE],
    };
    static TOP: Schema = Schema {
        name: "top",
        sql: "top;",
        models: &MODELS_A,
        dependencies: &[&MIDDLE, &BASE],
    };

    #[test]
    fn composes_dependencies_once_in_order() {
        assert_eq!(TOP.all_sql(), "base;\nmiddle;\ntop;");
    }
}
