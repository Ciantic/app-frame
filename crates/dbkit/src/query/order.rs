//! `ORDER BY` terms: a column and a direction, rendered after the filters.
//!
//! A term names a column of the model, so a query can only order by what it
//! reads, and only if the column declared `#[db(orderable)]`. A direction is a
//! keyword chosen from an enum, never a value, so ordering binds no parameter
//! and leaves the paging placeholders where they were.

use crate::Column;
use crate::query::ColumnRef;
use crate::query::Error;
use crate::query::MAX_ORDER;
use crate::query::capability::HasOrder;
use std::fmt;

/// The direction of one `ORDER BY` term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderDirection {
    Asc,
    Desc,
}

impl OrderDirection {
    /// The direction as the annotation-free vocabulary spells it: `asc`,
    /// `desc`.
    pub fn as_str(self) -> &'static str {
        match self {
            OrderDirection::Asc => "asc",
            OrderDirection::Desc => "desc",
        }
    }

    /// The direction of that name, or `None` when no direction is spelled so.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "asc" => Some(OrderDirection::Asc),
            "desc" => Some(OrderDirection::Desc),
            _ => None,
        }
    }

    /// The names of both directions, for an error that lists what is accepted.
    pub fn names() -> impl Iterator<Item = &'static str> {
        ["asc", "desc"].into_iter()
    }
}

/// One `ORDER BY` term: a column and the direction to sort it in.
///
/// The column handle has [`asc`](crate::ColumnRef::asc) and
/// [`desc`](crate::ColumnRef::desc), gated on `#[db(orderable)]`, so the usual
/// call site is `c.total.desc()`. [`asc`](Self::asc) and [`desc`](Self::desc)
/// build one from a bare column, which `render()` validates like any other
/// term.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OrderTerm<C: Column> {
    pub column: C,
    pub direction: OrderDirection,
}

impl<C: Column> OrderTerm<C> {
    pub const fn asc(column: C) -> Self {
        Self {
            column,
            direction: OrderDirection::Asc,
        }
    }

    pub const fn desc(column: C) -> Self {
        Self {
            column,
            direction: OrderDirection::Desc,
        }
    }
}

impl<C: Column> fmt::Debug for OrderTerm<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OrderTerm")
            .field("column", &self.column.name())
            .field("direction", &self.direction)
            .finish()
    }
}

impl<Tag: HasOrder<C>, C: Column, T> ColumnRef<Tag, C, T> {
    /// `ORDER BY "column" ASC`.
    pub const fn asc(self) -> OrderTerm<C> {
        OrderTerm::asc(self.column())
    }

    /// `ORDER BY "column" DESC`.
    pub const fn desc(self) -> OrderTerm<C> {
        OrderTerm::desc(self.column())
    }
}

/// Whether every term names an orderable column, and there are not too many.
pub(crate) fn check<C: Column>(order: &[OrderTerm<C>]) -> Result<(), Error> {
    if order.len() > MAX_ORDER {
        return Err(Error::OrderTerms { len: order.len() });
    }

    for term in order {
        if !term.column.orderable() {
            return Err(Error::NotOrderable {
                column: term.column.name(),
            });
        }
    }

    Ok(())
}

/// Reads one `order_by` entry: a column name, optionally followed by `asc` or
/// `desc`.
pub(crate) fn parse<C: Column>(value: &str) -> Result<OrderTerm<C>, Error> {
    let mut words = value.split_whitespace();
    let Some(name) = words.next() else {
        return Err(Error::InvalidOrder {
            value: value.to_owned(),
        });
    };

    let Some(column) = C::from_name(name) else {
        return Err(Error::UnknownColumn {
            name: name.to_owned(),
            known: C::all().iter().map(|column| column.name()).collect(),
        });
    };

    let direction = match words.next() {
        None => OrderDirection::Asc,
        Some(direction) => {
            OrderDirection::from_name(direction).ok_or_else(|| Error::UnknownDirection {
                name: direction.to_owned(),
            })?
        }
    };

    if words.next().is_some() {
        return Err(Error::InvalidOrder {
            value: value.to_owned(),
        });
    }

    Ok(OrderTerm { column, direction })
}
