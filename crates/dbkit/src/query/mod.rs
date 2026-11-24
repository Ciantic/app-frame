//! A `WHERE` clause as data: a query type tailored to one model's row source,
//! built from typed Rust values or parsed from succinct JSON, that renders to
//! the `(sql, params)` pair `sqlx` executes.
//!
//! Both faces converge on one [`Query<C>`], so a Rust-built query serializes
//! to the JSON that rebuilds it, and neither face formats a request string into
//! SQL: identifiers come from model metadata, and values are bound.

use crate::Column;
use crate::Columns;
use crate::FilterOperator;
use crate::Selectable;
use sqlx::AssertSqlSafe;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgRow;
use sqlx::postgres::Postgres;
use sqlx::query::QueryAs;
use std::fmt;

pub mod capability;
mod filter;
mod render;
mod serde;
mod value;

pub use filter::ColumnRef;
pub use filter::Combinator;
pub use filter::Filter;
pub use filter::Group;
pub use filter::IntoColumns;
pub use filter::Leaf;
pub use filter::Operand;
pub use render::Rendered;
pub use value::Bind;

/// The largest `LIMIT` a request may ask for.
pub const MAX_LIMIT: u32 = 1000;

/// How deeply `and`/`or` may nest.
pub const MAX_DEPTH: usize = 8;

/// How many filter nodes one query may hold.
pub const MAX_NODES: usize = 64;

/// How many values one list operand may hold.
pub const MAX_LIST: usize = 100;

/// The statement [`Query::build_selected`] returns: the SQL and its arguments,
/// ready to run, decoding into the model's select type.
pub type SelectedQuery<C> =
    QueryAs<'static, Postgres, <<C as Column>::Model as Selectable>::Select, PgArguments>;

/// One query of one model.
///
/// `filters` is the top-level conjunction: it appends, and it never nests, so
/// a group is only ever the explicit [`Filter::and`] or [`Filter::or`].
#[derive(Clone)]
pub struct Query<C: Column> {
    pub select: Option<Columns<C>>,
    pub filters: Vec<Filter<C>>,
    /// `None` means no `LIMIT`: every row the filters match.
    pub limit: Option<u32>,
    /// `None` and `0` both mean no `OFFSET`.
    pub offset: Option<u32>,
}

impl<C: Column> Query<C> {
    pub const fn new() -> Self {
        Self {
            select: None,
            filters: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    /// Selects the given columns instead of every column. Calling it again
    /// replaces the selection rather than adding to it.
    pub fn select(mut self, columns: impl IntoColumns<C>) -> Self {
        self.select = Some(columns.into_columns());
        self
    }

    /// Appends to the top-level conjunction.
    pub fn filters(mut self, filters: impl IntoIterator<Item = Filter<C>>) -> Self {
        self.filters.extend(filters);
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Flattens same-combinator nesting and drops single-child groups.
    ///
    /// The top level is a conjunction too, so an `and` group there joins it.
    pub fn normalize(mut self) -> Self {
        let mut filters = Vec::with_capacity(self.filters.len());

        for filter in self.filters {
            match filter.normalize() {
                Filter::Group(group) if group.combinator == Combinator::And => {
                    filters.extend(group.filters);
                }
                other => filters.push(other),
            }
        }

        self.filters = filters;
        self
    }

    /// Reads the JSON form: a `filters` object with `limit` and `offset`
    /// beside it.
    ///
    /// A syntactically broken payload is [`Error::Json`]; a rejected one keeps
    /// its own variant, so a handler can map it to a status.
    pub fn parse(json: &str) -> Result<Self, Error> {
        serde::parse(json)
    }

    /// The statement and its parameters, or the first problem the tree has.
    ///
    /// For a caller that wants the SQL text and the bound values: to log them,
    /// to hand them to its own executor, or to assert on them in a test.
    /// [`build`](Self::build) and [`build_selected`](Self::build_selected) are
    /// the same rendering taken straight to a query.
    pub fn render(&self) -> Result<Rendered, Error> {
        render::render(self)
    }

    /// The same statement as a `QueryAs`, decoding into the model.
    ///
    /// The SQL and its arguments are owned, so the value can be held, passed
    /// around and only then awaited:
    ///
    /// ```ignore
    /// let rows: Vec<Order> = Order::query().filters([c.status.eq("open")]).build()?.fetch_all(&pool).await?;
    /// ```
    ///
    /// `C::Model` must derive `sqlx::FromRow`, which the service models already
    /// do. The query has to read every column — no `select`, or one that names
    /// all of them — because a partial row cannot be decoded into the model;
    /// [`build_selected`](Self::build_selected) is the projection form.
    pub fn build(&self) -> Result<QueryAs<'static, Postgres, C::Model, PgArguments>, Error>
    where
        C::Model: for<'r> sqlx::FromRow<'r, PgRow> + Send + Unpin,
    {
        let rendered = self.render()?;

        if let Some(select) = &self.select
            && !C::all().iter().all(|column| select.contains(*column))
        {
            return Err(Error::ProjectedModel);
        }

        let Rendered { sql, arguments, .. } = rendered;

        Ok(sqlx::query_as_with::<_, C::Model, _>(
            AssertSqlSafe(sql),
            arguments,
        ))
    }

    /// The same statement, decoding into the model's select type.
    ///
    /// Use it for a sparse projection: decoding goes through `{Model}Select`,
    /// so a column the query left out is a
    /// [`Selected::Absent`](crate::Selected::Absent) field rather than a decode
    /// error.
    ///
    /// It needs an explicit `select`: a query that reads every column decodes
    /// into the model through [`build`](Self::build), and asking for the select
    /// type there is [`Error::MissingSelection`].
    ///
    /// ```ignore
    /// let rows = Order::query().select([c.total.column()]).build_selected()?.fetch_all(&pool).await?;
    /// ```
    pub fn build_selected(&self) -> Result<SelectedQuery<C>, Error>
    where
        C::Model: Selectable,
    {
        if self.select.is_none() {
            return Err(Error::MissingSelection);
        }

        let Rendered { sql, arguments, .. } = self.render()?;

        Ok(
            sqlx::query_as_with::<_, <C::Model as Selectable>::Select, _>(
                AssertSqlSafe(sql),
                arguments,
            ),
        )
    }
}

impl<C: Column> Default for Query<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Column> fmt::Debug for Query<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Query")
            .field("select", &self.select)
            .field("filters", &self.filters)
            .field("limit", &self.limit)
            .field("offset", &self.offset)
            .finish()
    }
}

impl<C: Column> PartialEq for Query<C> {
    fn eq(&self, other: &Self) -> bool {
        self.select == other.select
            && self.filters == other.filters
            && self.limit == other.limit
            && self.offset == other.offset
    }
}

impl<C: Column> Eq for Query<C> {}

/// Everything a request can be rejected for.
///
/// Display names the column and the problem; mapping a variant to an HTTP
/// status is the handler's job.
#[derive(Debug)]
pub enum Error {
    UnknownColumn {
        name: String,
        known: Vec<&'static str>,
    },
    UnknownOperator {
        name: String,
    },
    /// The column does not list the operator in its `query_ops`.
    OperatorNotAllowed {
        column: &'static str,
        operator: FilterOperator,
        allowed: &'static [FilterOperator],
    },
    /// The value has the wrong JSON shape for the operator.
    WrongShape {
        operator: FilterOperator,
        expected: &'static str,
        actual: String,
    },
    /// The value has the wrong type for the column.
    WrongType {
        column: &'static str,
        pg_type: String,
        actual: String,
    },
    /// The value has the right shape but does not parse, such as a bad uuid.
    InvalidValue {
        column: &'static str,
        pg_type: String,
        reason: String,
    },
    /// The column type has no JSON spelling.
    UnsupportedType {
        column: &'static str,
        pg_type: String,
    },
    /// A list operator was given an empty list.
    EmptyList {
        operator: FilterOperator,
    },
    ListTooLong {
        operator: FilterOperator,
        len: usize,
    },
    /// `eq` with `null`, which has one spelling: the bare key.
    NullNotAllowed {
        column: &'static str,
    },
    /// `is_null` and `is_not_null` take `true`.
    NotTrue {
        operator: FilterOperator,
    },
    /// `contains` and friends need an array column.
    ArrayRequired {
        column: &'static str,
        operator: FilterOperator,
    },
    /// `any` needs a scalar column; an array column has `overlaps`.
    ArrayAny {
        column: &'static str,
    },
    DuplicateKey {
        key: String,
    },
    EmptyGroup {
        combinator: Combinator,
    },
    /// The model has a column named `and` or `or`, which wins over the
    /// combinator key, so a group of that kind has no JSON spelling.
    CombinatorShadowed {
        combinator: Combinator,
    },
    /// A top-level key that is not `filters`, `limit` or `offset`.
    UnknownKey {
        key: String,
    },
    /// More than [`MAX_DEPTH`] nested groups.
    Depth,
    /// More than [`MAX_NODES`] filter nodes.
    Nodes,
    /// More than [`MAX_LIMIT`].
    Limit,
    /// The model is a fragment, which has no row source to read.
    NotATable,
    /// A selection with no columns, which is neither `SELECT *` nor valid SQL.
    EmptySelection,
    /// A `select` that leaves a column out, which [`Query::build`] cannot
    /// decode into the model.
    ProjectedModel,
    /// [`Query::build_selected`] on a query with no `select`, which
    /// [`Query::build`] decodes instead.
    MissingSelection,
    /// sqlx refused a value while the arguments were assembled.
    Encode(sqlx::Error),
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownColumn { name, known } => write!(
                formatter,
                "unknown column `{name}`; expected one of: {}",
                known.join(", ")
            ),
            Error::UnknownOperator { name } => write!(
                formatter,
                "operator `{name}` is not known; expected one of: {}",
                FilterOperator::names().collect::<Vec<_>>().join(", ")
            ),
            Error::OperatorNotAllowed {
                column,
                operator,
                allowed,
            } => {
                let allowed: Vec<&str> = allowed.iter().map(|allowed| allowed.as_str()).collect();

                write!(
                    formatter,
                    "operator `{}` is not allowed on column `{column}`; its query_ops are: {}",
                    operator.as_str(),
                    if allowed.is_empty() {
                        "none".to_owned()
                    } else {
                        allowed.join(", ")
                    },
                )
            }
            Error::WrongShape {
                operator,
                expected,
                actual,
            } => write!(
                formatter,
                "`{}` takes {expected}, got {actual}",
                operator.as_str()
            ),
            Error::WrongType {
                column,
                pg_type,
                actual,
            } => write!(
                formatter,
                "column `{column}` is {pg_type}, the value is {actual}"
            ),
            Error::InvalidValue {
                column,
                pg_type,
                reason,
            } => write!(formatter, "column `{column}` is {pg_type}, {reason}"),
            Error::UnsupportedType { column, pg_type } => write!(
                formatter,
                "column `{column}` is {pg_type}, which has no JSON filter value"
            ),
            Error::EmptyList { operator } => {
                write!(
                    formatter,
                    "`{}` needs at least one value",
                    operator.as_str()
                )
            }
            Error::ListTooLong { operator, len } => write!(
                formatter,
                "`{}` accepts at most {MAX_LIST} values, got {len}",
                operator.as_str()
            ),
            Error::NullNotAllowed { column } => {
                write!(formatter, "use `{column}`: null, or `{column}__neq`: null")
            }
            Error::NotTrue { operator } => write!(
                formatter,
                "`{}` takes `true`, not `false`",
                operator.as_str()
            ),
            Error::ArrayRequired { column, operator } => write!(
                formatter,
                "column `{column}` is not an array, so `{}` does not apply; use `any` instead",
                operator.as_str()
            ),
            Error::ArrayAny { column } => write!(
                formatter,
                "column `{column}` is an array, so `any` does not apply; use `overlaps`, \
                 `contains_all` or `contains` instead"
            ),
            Error::DuplicateKey { key } => write!(formatter, "duplicate key `{key}`"),
            Error::EmptyGroup { combinator } => write!(
                formatter,
                "`{}` needs at least one filter",
                combinator.as_str()
            ),
            Error::CombinatorShadowed { combinator } => write!(
                formatter,
                "the model has a column named `{}`, which wins over the key, so a `{}` group has \
                 no JSON form",
                combinator.as_str(),
                combinator.as_str(),
            ),
            Error::UnknownKey { key } => write!(
                formatter,
                "unknown key `{key}`; a query takes `select`, `filters`, `limit` and `offset`"
            ),
            Error::Depth => write!(formatter, "filters nest deeper than {MAX_DEPTH} levels"),
            Error::Nodes => write!(
                formatter,
                "the query has more than {MAX_NODES} filter nodes"
            ),
            Error::Limit => write!(formatter, "`limit` must be at most {MAX_LIMIT}"),
            Error::NotATable => {
                formatter.write_str("the model is a `#[db(fragment)]`, which has no table to query")
            }
            Error::EmptySelection => formatter.write_str(
                "a selection needs at least one column; omit `select` to read every column",
            ),
            Error::ProjectedModel => formatter.write_str(
                "`build` decodes every column; the query selects a subset, so call \
                 `build_selected` or omit `select`",
            ),
            Error::MissingSelection => formatter.write_str(
                "`build_selected` decodes a projection; the query has no `select`, so call \
                 `build` or name the columns",
            ),
            Error::Encode(error) => error.fmt(formatter),
            Error::Json(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Encode(error) => Some(error),
            Error::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Error::Encode(error)
    }
}
