//! Applying a batch of patches: one `UPDATE` per entry, all in one transaction.

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

/// One written column of a patch or an insert. `None` means SQL `NULL`.
#[derive(Debug, Clone, Copy)]
pub struct Assignment<'a> {
    pub column: &'static str,
    pub value: Option<&'a dyn Bind>,
}

/// The update view of a patch from `#[derive(DbPatch)]`; it pairs with `TableMeta::primary_key` and `version`.
pub trait PatchRow {
    type Model: Model;

    /// Changed business columns, in declaration order.
    fn assignments(&self) -> Vec<Assignment<'_>>;

    /// Primary key values, in `TableMeta::primary_key` order.
    fn key(&self) -> Vec<&dyn Bind>;

    /// The version the update expects, when the model has one.
    fn version(&self) -> Option<&dyn Bind>;
}

/// A batch of patches: one `UPDATE` per entry in one transaction, and every entry must update exactly one row.
pub struct PatchQuery<P: PatchRow> {
    entries: Vec<P>,
}

/// One rendered `UPDATE` of a patch entry.
#[derive(Debug)]
pub struct RenderedPatch {
    pub sql: String,
    pub arguments: PgArguments,
    /// The bound values as JSON, for assertions and logs.
    pub values: Vec<serde_json::Value>,
}

impl<P: PatchRow> PatchQuery<P> {
    pub fn new(entries: impl IntoIterator<Item = P>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// One statement per entry, in entry order.
    pub fn render(&self) -> Result<Vec<RenderedPatch>, PatchError> {
        let meta = match P::Model::meta() {
            Meta::Table(meta) => meta,
            Meta::SubQuery(_) | Meta::Fragment(_) => return Err(PatchError::NotATable),
        };

        self.entries
            .iter()
            .enumerate()
            .map(|(index, patch)| render_entry(meta, patch, index))
            .collect()
    }

    /// Renders first, then runs one transaction (`&mut Transaction` nests a savepoint); a failure rolls all back.
    pub async fn execute<'c, A>(self, acquirer: A) -> Result<(), PatchError>
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
                return Err(PatchError::NotUpdated { index });
            }
        }

        transaction.commit().await?;

        Ok(())
    }
}

/// Everything a patch batch can be rejected for.
#[derive(Debug)]
pub enum PatchError {
    /// An entry has no changed business column.
    Empty { index: usize },
    /// The generated key values do not match `TableMeta::primary_key`.
    KeyColumns { expected: usize, actual: usize },
    /// `TableMeta::version` and `PatchRow::version()` disagree.
    VersionColumns,
    /// A patch model is a fragment or subquery and has no table.
    NotATable,
    /// `rows_affected` was not exactly one on this entry.
    NotUpdated { index: usize },
    /// Encode, execute, savepoint or commit failure.
    Sqlx(sqlx::Error),
}

impl fmt::Display for PatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Empty { index } => {
                write!(formatter, "patch entry {index} changes no column")
            }
            PatchError::KeyColumns { expected, actual } => write!(
                formatter,
                "patch carries {actual} primary key values, the model has {expected}"
            ),
            PatchError::VersionColumns => formatter
                .write_str("the model's version column and the patch's expected version disagree"),
            PatchError::NotATable => {
                formatter.write_str("a patch batch needs a table model, not a fragment or subquery")
            }
            PatchError::NotUpdated { index } => write!(
                formatter,
                "patch entry {index} did not update exactly one row; the row is missing or its \
                 version changed"
            ),
            PatchError::Sqlx(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PatchError::Sqlx(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for PatchError {
    fn from(error: sqlx::Error) -> Self {
        PatchError::Sqlx(error)
    }
}

fn render_entry<P: PatchRow>(
    meta: &TableMeta,
    patch: &P,
    index: usize,
) -> Result<RenderedPatch, PatchError> {
    let assignments = patch.assignments();
    if assignments.is_empty() {
        return Err(PatchError::Empty { index });
    }

    let key = patch.key();
    if key.len() != meta.primary_key.len() {
        return Err(PatchError::KeyColumns {
            expected: meta.primary_key.len(),
            actual: key.len(),
        });
    }

    let expected_version = match (meta.version, patch.version()) {
        (Some(_), Some(version)) => Some(version),
        (None, None) => None,
        _ => return Err(PatchError::VersionColumns),
    };

    let mut renderer = PatchRenderer::default();

    renderer.sql.push_str("UPDATE ");
    push_identifier(&mut renderer.sql, meta.table);
    renderer.sql.push_str(" SET ");

    for (position, assignment) in assignments.iter().enumerate() {
        if position > 0 {
            renderer.sql.push_str(", ");
        }

        push_identifier(&mut renderer.sql, assignment.column);
        renderer.sql.push_str(" = ");
        match assignment.value {
            Some(value) => renderer.push(value)?,
            None => renderer.sql.push_str("NULL"),
        }
    }

    if let Some(version) = meta.version {
        renderer.sql.push_str(", ");
        push_identifier(&mut renderer.sql, version);
        renderer.sql.push_str(" = ");
        push_identifier(&mut renderer.sql, version);
        renderer.sql.push_str(" + 1");
    }

    if let Some(updated_at) = meta.updated_at {
        renderer.sql.push_str(", ");
        push_identifier(&mut renderer.sql, updated_at);
        renderer.sql.push_str(" = now()");
    }

    renderer.sql.push_str(" WHERE ");

    for (position, (column, value)) in meta.primary_key.iter().zip(key).enumerate() {
        if position > 0 {
            renderer.sql.push_str(" AND ");
        }

        push_identifier(&mut renderer.sql, column);
        renderer.sql.push_str(" = ");
        renderer.push(value)?;
    }

    if let (Some(version), Some(expected)) = (meta.version, expected_version) {
        renderer.sql.push_str(" AND ");
        push_identifier(&mut renderer.sql, version);
        renderer.sql.push_str(" = ");
        renderer.push(expected)?;
    }

    Ok(RenderedPatch {
        sql: renderer.sql,
        arguments: renderer.arguments,
        values: renderer.values,
    })
}

#[derive(Default)]
struct PatchRenderer {
    sql: String,
    arguments: PgArguments,
    values: Vec<serde_json::Value>,
    placeholders: usize,
}

impl PatchRenderer {
    fn push(&mut self, value: &dyn Bind) -> Result<(), PatchError> {
        self.placeholders += 1;
        self.sql.push('$');
        self.sql.push_str(&self.placeholders.to_string());
        value.bind(&mut self.arguments)?;
        self.values.push(value.as_json());

        Ok(())
    }
}
