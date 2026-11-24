//! Rendering a [`Query`] to SQL and bound parameters.

use crate::Column;
use crate::FilterOperator;
use crate::query::Bind;
use crate::query::Combinator;
use crate::query::Error;
use crate::query::Filter;
use crate::query::MAX_DEPTH;
use crate::query::MAX_LIMIT;
use crate::query::MAX_LIST;
use crate::query::MAX_NODES;
use crate::query::Operand;
use crate::query::Query;
use crate::query::value::is_array;
use sqlx::postgres::PgArguments;
use std::sync::Arc;

/// The statement `sqlx` executes, and the values it was rendered from.
///
/// What [`Query::render`] returns: the SQL text and its arguments, for a caller
/// that logs them, hands them to its own executor, or asserts on them in a
/// test. [`Query::build`] takes the same pair straight to a `QueryAs`.
///
/// [`Query::build`]: crate::Query::build
/// [`Query::render`]: crate::Query::render
pub struct Rendered {
    pub sql: String,
    /// Assembled ahead of the query, which `query_as_with` then takes.
    pub arguments: PgArguments,
    /// The bound values in placeholder order, for assertions and logs.
    pub values: Vec<Arc<dyn Bind>>,
}

impl std::fmt::Debug for Rendered {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Rendered")
            .field("sql", &self.sql)
            .field("values", &self.values_json())
            .finish_non_exhaustive()
    }
}

impl Rendered {
    /// The same values as JSON, for a readable assertion.
    pub fn values_json(&self) -> Vec<serde_json::Value> {
        self.values.iter().map(|value| value.as_json()).collect()
    }
}

/// How many values an operator binds, and in what shape.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arity {
    One,
    /// One array value.
    List,
    Two,
    None,
}

fn arity(operator: FilterOperator) -> Arity {
    match operator {
        FilterOperator::Any
        | FilterOperator::NotAny
        | FilterOperator::Overlaps
        | FilterOperator::NotOverlaps
        | FilterOperator::ContainsAll
        | FilterOperator::NotContainsAll => Arity::List,
        FilterOperator::Between | FilterOperator::NotBetween => Arity::Two,
        FilterOperator::IsNull | FilterOperator::IsNotNull => Arity::None,
        _ => Arity::One,
    }
}

/// The operators that compare against an array column.
fn needs_array_column(operator: FilterOperator) -> bool {
    matches!(
        operator,
        FilterOperator::Contains
            | FilterOperator::NotContains
            | FilterOperator::Overlaps
            | FilterOperator::NotOverlaps
            | FilterOperator::ContainsAll
            | FilterOperator::NotContainsAll
    )
}

pub(crate) fn render<C: Column>(query: &Query<C>) -> Result<Rendered, Error> {
    let table = query.table()?;
    let mut renderer = Renderer::default();

    renderer.sql.push_str("SELECT ");
    match &query.select {
        Some(columns) => {
            if columns.is_empty() {
                return Err(Error::EmptySelection);
            }

            for (index, name) in columns.names().enumerate() {
                if index > 0 {
                    renderer.sql.push_str(", ");
                }
                push_identifier(&mut renderer.sql, name);
            }
        }
        None => renderer.sql.push('*'),
    }

    renderer.sql.push_str(" FROM ");
    push_identifier(&mut renderer.sql, table);

    if !query.filters.is_empty() {
        renderer.sql.push_str(" WHERE ");
        renderer.level(&query.filters, Combinator::And, 0)?;
    }

    if let Some(limit) = query.limit {
        if limit > MAX_LIMIT {
            return Err(Error::Limit);
        }

        renderer.sql.push_str(" LIMIT ");
        renderer.push(Arc::new(i64::from(limit)))?;
    }

    if let Some(offset) = query.offset.filter(|offset| *offset != 0) {
        renderer.sql.push_str(" OFFSET ");
        renderer.push(Arc::new(i64::from(offset)))?;
    }

    Ok(Rendered {
        sql: renderer.sql,
        arguments: renderer.arguments,
        values: renderer.values,
    })
}

#[derive(Default)]
struct Renderer {
    sql: String,
    arguments: PgArguments,
    values: Vec<Arc<dyn Bind>>,
    placeholders: usize,
    nodes: usize,
}

impl Renderer {
    fn level<C: Column>(
        &mut self,
        filters: &[Filter<C>],
        combinator: Combinator,
        depth: usize,
    ) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Depth);
        }

        for (index, filter) in filters.iter().enumerate() {
            if index > 0 {
                self.sql.push_str(match combinator {
                    Combinator::And => " AND ",
                    Combinator::Or => " OR ",
                });
            }

            self.filter(filter, depth)?;
        }

        Ok(())
    }

    fn filter<C: Column>(&mut self, filter: &Filter<C>, depth: usize) -> Result<(), Error> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(Error::Nodes);
        }

        match filter {
            Filter::Leaf(leaf) => self.leaf(leaf),
            Filter::Group(group) => {
                if group.filters.is_empty() {
                    return Err(Error::EmptyGroup {
                        combinator: group.combinator,
                    });
                }

                self.sql.push('(');
                self.level(&group.filters, group.combinator, depth + 1)?;
                self.sql.push(')');

                Ok(())
            }
        }
    }

    fn leaf<C: Column>(&mut self, leaf: &crate::query::Leaf<C>) -> Result<(), Error> {
        let operator = leaf.operator;
        let column = leaf.column.name();

        self.check(leaf)?;

        match (&leaf.operand, operator) {
            (Operand::None, FilterOperator::IsNull) => {
                self.column(column);
                self.sql.push_str(" IS NULL");
            }
            (Operand::None, FilterOperator::IsNotNull) => {
                self.column(column);
                self.sql.push_str(" IS NOT NULL");
            }
            // `neq` with `null` is the short spelling of `is_not_null`.
            (Operand::None, FilterOperator::Neq) => {
                self.column(column);
                self.sql.push_str(" IS NOT NULL");
            }
            (Operand::One(value), FilterOperator::Contains) => {
                self.push(Arc::clone(value))?;
                self.sql.push_str(" = ANY(");
                self.column(column);
                self.sql.push(')');
            }
            (Operand::One(value), FilterOperator::NotContains) => {
                self.sql.push_str("NOT (");
                self.push(Arc::clone(value))?;
                self.sql.push_str(" = ANY(");
                self.column(column);
                self.sql.push_str("))");
            }
            (Operand::One(value), operator) => {
                self.column(column);
                self.sql.push(' ');
                self.sql.push_str(symbol(operator));
                self.sql.push(' ');
                self.push(Arc::clone(value))?;
            }
            (Operand::Two(low, high), FilterOperator::NotBetween) => {
                self.column(column);
                self.sql.push_str(" NOT BETWEEN ");
                self.push(Arc::clone(low))?;
                self.sql.push_str(" AND ");
                self.push(Arc::clone(high))?;
            }
            (Operand::Two(low, high), _) => {
                self.column(column);
                self.sql.push_str(" BETWEEN ");
                self.push(Arc::clone(low))?;
                self.sql.push_str(" AND ");
                self.push(Arc::clone(high))?;
            }
            (Operand::List { values, .. }, FilterOperator::Any) => {
                self.column(column);
                self.sql.push_str(" = ANY(");
                self.push(Arc::clone(values))?;
                self.sql.push(')');
            }
            (Operand::List { values, .. }, FilterOperator::NotAny) => {
                self.column(column);
                self.sql.push_str(" <> ALL(");
                self.push(Arc::clone(values))?;
                self.sql.push(')');
            }
            (Operand::List { values, .. }, FilterOperator::Overlaps) => {
                self.column(column);
                self.sql.push_str(" && ");
                self.push(Arc::clone(values))?;
            }
            (Operand::List { values, .. }, FilterOperator::NotOverlaps) => {
                self.sql.push_str("NOT (");
                self.column(column);
                self.sql.push_str(" && ");
                self.push(Arc::clone(values))?;
                self.sql.push(')');
            }
            (Operand::List { values, .. }, FilterOperator::ContainsAll) => {
                self.column(column);
                self.sql.push_str(" @> ");
                self.push(Arc::clone(values))?;
            }
            (Operand::List { values, .. }, FilterOperator::NotContainsAll) => {
                self.sql.push_str("NOT (");
                self.column(column);
                self.sql.push_str(" @> ");
                self.push(Arc::clone(values))?;
                self.sql.push(')');
            }
            (operand, operator) => {
                return Err(Error::WrongShape {
                    operator,
                    expected: match arity(operator) {
                        Arity::One => "one value",
                        Arity::List => "a non-empty list",
                        Arity::Two => "two values",
                        Arity::None => "no value",
                    },
                    actual: operand.kind().to_owned(),
                });
            }
        }

        Ok(())
    }

    /// Rejects an operator the column never declared, an operator that does not
    /// fit the column type, and an operand of the wrong arity.
    fn check<C: Column>(&self, leaf: &crate::query::Leaf<C>) -> Result<(), Error> {
        let operator = leaf.operator;
        let column = leaf.column.name();

        if !operator.is_nullary() && !leaf.column.query_ops().contains(&operator) {
            return Err(Error::OperatorNotAllowed {
                column,
                operator,
                allowed: leaf.column.query_ops(),
            });
        }

        if needs_array_column(operator) && !is_array(leaf.column.pg_type()) {
            return Err(Error::ArrayRequired { column, operator });
        }

        if matches!(operator, FilterOperator::Any | FilterOperator::NotAny)
            && is_array(leaf.column.pg_type())
        {
            return Err(Error::ArrayAny { column });
        }

        match (arity(operator), &leaf.operand) {
            (Arity::One, Operand::One(_)) | (Arity::None, Operand::None) => Ok(()),
            // The two spellings of "there is no value" that are not nullary.
            (Arity::One, Operand::None) if operator == FilterOperator::Neq => Ok(()),
            (Arity::One, Operand::None) if operator == FilterOperator::Eq => {
                Err(Error::NullNotAllowed { column })
            }
            (Arity::Two, Operand::Two(_, _)) => Ok(()),
            (Arity::List, Operand::List { len: 0, .. }) => Err(Error::EmptyList { operator }),
            (Arity::List, Operand::List { len, .. }) if *len > MAX_LIST => {
                Err(Error::ListTooLong {
                    operator,
                    len: *len,
                })
            }
            (Arity::List, Operand::List { .. }) => Ok(()),
            (expected, operand) => Err(Error::WrongShape {
                operator,
                expected: match expected {
                    Arity::One => "one value",
                    Arity::List => "a non-empty list",
                    Arity::Two => "two values",
                    Arity::None => "no value",
                },
                actual: operand.kind().to_owned(),
            }),
        }
    }

    fn column(&mut self, column: &str) {
        push_identifier(&mut self.sql, column);
    }

    fn push(&mut self, value: Arc<dyn Bind>) -> Result<(), Error> {
        self.placeholders += 1;
        self.sql.push('$');
        self.sql.push_str(&self.placeholders.to_string());
        value.bind(&mut self.arguments)?;
        self.values.push(value);

        Ok(())
    }
}

fn symbol(operator: FilterOperator) -> &'static str {
    match operator {
        FilterOperator::Eq => "=",
        FilterOperator::Neq => "<>",
        FilterOperator::Gt => ">",
        FilterOperator::Gte => ">=",
        FilterOperator::Lt => "<",
        FilterOperator::Lte => "<=",
        FilterOperator::Like => "LIKE",
        FilterOperator::ILike => "ILIKE",
        FilterOperator::NotLike => "NOT LIKE",
        FilterOperator::NotILike => "NOT ILIKE",
        _ => unreachable!("only the symbolic and the pattern operators reach here"),
    }
}

/// Quotes an identifier, doubling an embedded `"`.
fn push_identifier(sql: &mut String, name: &str) {
    sql.push('"');

    for character in name.chars() {
        if character == '"' {
            sql.push('"');
        }
        sql.push(character);
    }

    sql.push('"');
}
