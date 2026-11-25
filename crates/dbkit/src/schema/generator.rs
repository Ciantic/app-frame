use crate::ErasedColumn;
use crate::ForeignKeyAction;
use crate::TableMeta;
use crate::Trigger;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub(crate) fn generate_body(
    owned: &[TableMeta],
    known: &[TableMeta],
) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    let mut known_tables = BTreeSet::new();
    for meta in known {
        if !known_tables.insert(meta.table) {
            problems.push(format!(
                "table \"{}\" is declared more than once",
                meta.table
            ));
        }
    }
    let mut owned_tables = BTreeSet::new();
    let mut index_names = BTreeSet::new();
    for meta in owned {
        if !owned_tables.insert(meta.table) {
            problems.push(format!(
                "table \"{}\" is declared more than once",
                meta.table
            ));
        }
        for index in meta.indexes {
            if !index_names.insert(index.name) {
                problems.push(format!(
                    "index name \"{}\" is declared more than once",
                    index.name
                ));
            }
        }
    }

    let mut statements = Vec::new();
    for meta in owned {
        statements.push(create_table(meta, &mut problems));
    }
    for meta in owned {
        statements.extend(foreign_keys(meta, &known_tables, &mut problems));
        statements.extend(indexes(meta, &mut problems));
    }
    statements.extend(trigger_statements(owned, &owned_tables, &mut problems));

    if !problems.is_empty() {
        return Err(problems);
    }
    if statements.is_empty() {
        return Ok(String::new());
    }

    Ok(statements.join("\n\n"))
}

fn create_table(meta: &TableMeta, problems: &mut Vec<String>) -> String {
    let mut definitions = Vec::new();
    for column in meta.columns {
        definitions.push(column_definition(*column, problems));
    }
    let primary_key = quoted_columns(meta.primary_key, problems);
    definitions.push(format!(
        "    constraint {} primary key ({primary_key})",
        identifier(&format!("{}_pkey", meta.table), problems),
    ));
    for key in meta.unique_keys {
        let name = format!("{}_{}_key", meta.table, key.columns.join("_"));
        definitions.push(format!(
            "    constraint {} unique ({})",
            identifier(&name, problems),
            quoted_columns(key.columns, problems),
        ));
    }

    format!(
        "create table {} (\n{}\n);",
        identifier(meta.table, problems),
        definitions.join(",\n"),
    )
}

fn column_definition(column: &dyn ErasedColumn, problems: &mut Vec<String>) -> String {
    let mut definition = format!(
        "    {} {}",
        identifier(column.name(), problems),
        column.pg_type(),
    );
    if let Some(expression) = column.generated() {
        definition.push_str(&format!(" generated always as ({expression}) virtual"));
    }
    if !column.nullable() {
        definition.push_str(" not null");
    }
    if column.generated().is_none()
        && let Some(default) = column.pg_default()
    {
        definition.push_str(&format!(" default {default}"));
    }

    definition
}

fn foreign_keys(
    meta: &TableMeta,
    known: &BTreeSet<&str>,
    problems: &mut Vec<String>,
) -> Vec<String> {
    let mut statements = Vec::new();

    for foreign_key in meta.foreign_keys {
        if !known.contains(foreign_key.table) {
            problems.push(format!(
                "{}: foreign key {} references table \"{}\", which is not in the composition",
                meta.table,
                columns_text(foreign_key.columns),
                foreign_key.table,
            ));
        }

        let name = format!("{}_{}_fkey", meta.table, foreign_key.columns.join("_"));
        let mut statement = format!(
            "alter table {} add constraint {} foreign key ({}) references {} ({})",
            identifier(meta.table, problems),
            identifier(&name, problems),
            quoted_columns(foreign_key.columns, problems),
            identifier(foreign_key.table, problems),
            quoted_columns(foreign_key.referenced_columns, problems),
        );
        if foreign_key.on_delete != ForeignKeyAction::NoAction {
            statement.push_str(&format!(" on delete {}", foreign_key.on_delete.as_str()));
        }
        if foreign_key.on_update != ForeignKeyAction::NoAction {
            statement.push_str(&format!(" on update {}", foreign_key.on_update.as_str()));
        }
        statement.push(';');
        statements.push(statement);
    }

    statements
}

fn indexes(meta: &TableMeta, problems: &mut Vec<String>) -> Vec<String> {
    meta.indexes
        .iter()
        .map(|index| {
            format!(
                "create index {} on {} ({});",
                identifier(index.name, problems),
                identifier(meta.table, problems),
                quoted_columns(index.columns, problems),
            )
        })
        .collect()
}

fn trigger_statements(
    owned: &[TableMeta],
    owned_tables: &BTreeSet<&str>,
    problems: &mut Vec<String>,
) -> Vec<String> {
    let mut definitions: BTreeMap<(&str, &str), &Trigger> = BTreeMap::new();
    for meta in owned {
        for trigger in meta.trigger_definitions {
            if !owned_tables.contains(trigger.table) {
                problems.push(format!(
                    "trigger `{}` is installed on table \"{}\", which this service does not own",
                    trigger.name, trigger.table,
                ));
                continue;
            }
            match definitions.get(&(trigger.table, trigger.name)) {
                Some(existing) if *existing != trigger => problems.push(format!(
                    "trigger `{}` on \"{}\" has conflicting definitions",
                    trigger.name, trigger.table,
                )),
                Some(_) => {}
                None => {
                    definitions.insert((trigger.table, trigger.name), trigger);
                }
            }
        }
    }

    for meta in owned {
        for reference in meta.triggers {
            if owned_tables.contains(reference.table)
                && !definitions.contains_key(&(reference.table, reference.name))
            {
                problems.push(format!(
                    "trigger `{}` on \"{}\" is declared but has no definition",
                    reference.name, reference.table,
                ));
            }
        }
    }

    let mut functions: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for trigger in definitions.values() {
        match functions.get(trigger.function) {
            Some((language, body)) if *language != trigger.language || *body != trigger.body => {
                problems.push(format!(
                    "trigger function \"{}\" has conflicting definitions",
                    trigger.function,
                ));
            }
            Some(_) => {}
            None => {
                functions.insert(trigger.function, (trigger.language, trigger.body));
            }
        }
    }

    let mut statements = Vec::new();
    for (name, (language, body)) in &functions {
        let tag = dollar_tag(body);
        statements.push(format!(
            "create function {}() returns trigger language {} as {tag}\n{}\n{tag};",
            identifier(name, problems),
            identifier(language, problems),
            body.trim_end(),
        ));
    }
    for trigger in definitions.values() {
        let events: Vec<&str> = trigger.events.iter().map(|event| event.as_str()).collect();
        let mut statement = format!(
            "create trigger {}\n{} {} on {}\nfor each {}\n",
            identifier(trigger.name, problems),
            trigger.timing.as_str(),
            events.join(" or "),
            identifier(trigger.table, problems),
            trigger.level.as_str(),
        );
        if let Some(condition) = trigger.condition {
            statement.push_str(&format!("when ({condition})\n"));
        }
        statement.push_str(&format!(
            "execute function {}();",
            identifier(trigger.function, problems),
        ));
        statements.push(statement);
    }

    statements
}

fn quoted_columns(columns: &[&str], problems: &mut Vec<String>) -> String {
    columns
        .iter()
        .map(|column| identifier(column, problems))
        .collect::<Vec<_>>()
        .join(", ")
}

fn columns_text(columns: &[&str]) -> String {
    format!("({})", columns.join(", "))
}

fn identifier(name: &str, problems: &mut Vec<String>) -> String {
    if name.len() > 63 {
        problems.push(format!("identifier `{name}` is longer than 63 bytes"));
    }

    format!("\"{}\"", name.replace('"', "\"\""))
}

fn dollar_tag(body: &str) -> String {
    if !body.contains("$$") {
        return "$$".to_owned();
    }

    let mut index = 0;
    loop {
        let tag = if index == 0 {
            "$dbkit$".to_owned()
        } else {
            format!("$dbkit{index}$")
        };
        if !body.contains(&tag) {
            return tag;
        }
        index += 1;
    }
}
