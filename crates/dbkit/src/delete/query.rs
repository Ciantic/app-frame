//! Deleting rows by key: one `DELETE` per entry, all in one transaction.

use crate::Meta;
use crate::Model;
use crate::TableMeta;
use crate::query::Bind;
use crate::query::render::push_identifier;
use sqlx::Acquire;
use sqlx::AssertSqlSafe;
use sqlx::Postgres;
use sqlx::postgres::PgArguments;
use std::fmt;

/// The delete view of a key from `#[derive(DbDelete)]`; it pairs with `TableMeta::primary_key` and `version`.
pub trait DeleteKey: crate::TenantRow {
    type Model: Model;

    /// Primary key values, in `TableMeta::primary_key` order.
    fn key(&self) -> Vec<&dyn Bind>;

    /// The version the delete expects, when the model has one.
    fn version(&self) -> Option<&dyn Bind>;
}

/// A batch of deletes: one `DELETE` per entry in one transaction, and every entry must delete exactly one row.
pub struct DeleteQuery<K: DeleteKey> {
    entries: Vec<K>,
}

/// One rendered `DELETE` of a key entry.
#[derive(Debug)]
pub struct RenderedDelete {
    pub sql: String,
    pub arguments: PgArguments,
    /// The bound values as JSON, for assertions and logs.
    pub values: Vec<serde_json::Value>,
}

impl<K: DeleteKey> DeleteQuery<K> {
    pub fn new(entries: impl IntoIterator<Item = K>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// One statement per entry, in entry order.
    pub fn render(&self) -> Result<Vec<RenderedDelete>, DeleteError> {
        let meta = match K::Model::meta() {
            Meta::Table(meta) => meta,
            Meta::SubQuery(_) | Meta::Fragment(_) => return Err(DeleteError::NotATable),
        };

        self.entries
            .iter()
            .map(|key| render_entry(meta, key))
            .collect()
    }

    /// Renders first, then runs one transaction (`&mut Transaction` nests a savepoint); a failure rolls all back.
    pub async fn execute<'c, A>(self, acquirer: A) -> Result<(), DeleteError>
    where
        A: Acquire<'c, Database = Postgres>,
    {
        let statements = self.render()?;
        let mut transaction = acquirer.begin().await?;

        for (index, statement) in statements.into_iter().enumerate() {
            let result = sqlx::query_with(AssertSqlSafe(statement.sql), statement.arguments)
                .execute(&mut *transaction)
                .await?;

            if result.rows_affected() != 1 {
                return Err(DeleteError::NotDeleted { index });
            }
        }

        transaction.commit().await?;

        Ok(())
    }
}

/// Everything a delete batch can be rejected for.
#[derive(Debug)]
pub enum DeleteError {
    /// The generated key values do not match `TableMeta::primary_key`.
    KeyColumns { expected: usize, actual: usize },
    /// `TableMeta::version` and `DeleteKey::version()` disagree.
    VersionColumns,
    /// `TableMeta::tenant` and `TenantRow::tenant()` disagree.
    TenantColumns,
    /// A delete model is a fragment or subquery and has no table.
    NotATable,
    /// `rows_affected` was not exactly one on this entry.
    NotDeleted { index: usize },
    /// Encode, execute, savepoint or commit failure.
    Sqlx(sqlx::Error),
}

impl fmt::Display for DeleteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeleteError::KeyColumns { expected, actual } => write!(
                formatter,
                "delete carries {actual} primary key values, the model has {expected}"
            ),
            DeleteError::VersionColumns => formatter
                .write_str("the model's version column and the key's expected version disagree"),
            DeleteError::TenantColumns => {
                formatter.write_str("the model's tenant column and the key's tenant value disagree")
            }
            DeleteError::NotATable => formatter
                .write_str("a delete batch needs a table model, not a fragment or subquery"),
            DeleteError::NotDeleted { index } => write!(
                formatter,
                "delete entry {index} did not delete exactly one row; the row is missing or its \
                 version changed, and the batch was rolled back"
            ),
            DeleteError::Sqlx(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DeleteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DeleteError::Sqlx(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for DeleteError {
    fn from(error: sqlx::Error) -> Self {
        DeleteError::Sqlx(error)
    }
}

fn render_entry<K: DeleteKey>(meta: &TableMeta, key: &K) -> Result<RenderedDelete, DeleteError> {
    let key_values = key.key();
    if key_values.len() != meta.primary_key.len() {
        return Err(DeleteError::KeyColumns {
            expected: meta.primary_key.len(),
            actual: key_values.len(),
        });
    }

    let expected_version = match (meta.version, key.version()) {
        (Some(_), Some(version)) => Some(version),
        (None, None) => None,
        _ => return Err(DeleteError::VersionColumns),
    };

    let mut renderer = DeleteRenderer::default();

    renderer.sql.push_str("DELETE FROM ");
    push_identifier(&mut renderer.sql, meta.table);
    renderer.sql.push_str(" WHERE ");

    for (position, (column, value)) in meta.primary_key.iter().zip(key_values).enumerate() {
        if position > 0 {
            renderer.sql.push_str(" AND ");
        }

        push_identifier(&mut renderer.sql, column);
        renderer.sql.push_str(" = ");
        renderer.push(value)?;
    }

    if let Some(tenant_column) = meta.tenant
        && !meta.primary_key.contains(&tenant_column)
    {
        let tenant = key.tenant().ok_or(DeleteError::TenantColumns)?;

        renderer.sql.push_str(" AND ");
        push_identifier(&mut renderer.sql, tenant_column);
        renderer.sql.push_str(" = ");
        renderer.push(tenant)?;
    }

    if let (Some(version), Some(expected)) = (meta.version, expected_version) {
        renderer.sql.push_str(" AND ");
        push_identifier(&mut renderer.sql, version);
        renderer.sql.push_str(" = ");
        renderer.push(expected)?;
    }

    Ok(RenderedDelete {
        sql: renderer.sql,
        arguments: renderer.arguments,
        values: renderer.values,
    })
}

#[derive(Default)]
struct DeleteRenderer {
    sql: String,
    arguments: PgArguments,
    values: Vec<serde_json::Value>,
    placeholders: usize,
}

impl DeleteRenderer {
    fn push(&mut self, value: &dyn Bind) -> Result<(), DeleteError> {
        self.placeholders += 1;
        self.sql.push('$');
        self.sql.push_str(&self.placeholders.to_string());
        value.bind(&mut self.arguments)?;
        self.values.push(value.as_json());

        Ok(())
    }
}
