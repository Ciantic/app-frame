//! Selectable columns of a model.

use crate::model::ErasedColumn;
use crate::model::FilterOperator;
use crate::model::Model;
use indexmap::IndexSet;
use serde::Deserialize;
use serde::Serialize;
use std::hash::Hash;

/// One selectable column of a model: the `<Model>Column` variant and its `TableMeta` entry come from one source.
pub trait Column: Copy + Ord + Hash + std::fmt::Debug + Send + Sync + 'static {
    /// The model the column belongs to.
    type Model: Model;

    /// Every column, in declaration order, with a flattened field's columns at the field's position.
    fn all() -> &'static [Self];

    /// The column name as the database spells it.
    fn name(self) -> &'static str;

    /// Whether the column may appear in `ORDER BY`, as declared by `#[db(orderable)]`.
    fn orderable(self) -> bool;

    /// The column of that name, or `None` when the model has no such column.
    fn from_name(name: &str) -> Option<Self>;

    /// The column type as DDL spells it: `uuid`, `text`, `varchar(255)`.
    fn pg_type(self) -> &'static str;

    /// The column default as DDL spells it, or `None` when the column has none.
    fn pg_default(self) -> Option<&'static str>;

    /// Whether the column is nullable.
    fn nullable(self) -> bool;

    /// The comparisons the column may be queried with, in declaration order.
    fn query_ops(self) -> &'static [FilterOperator];
}

impl<C: Column> ErasedColumn for C {
    fn name(&self) -> &'static str {
        Column::name(*self)
    }

    fn orderable(&self) -> bool {
        Column::orderable(*self)
    }

    fn pg_type(&self) -> &'static str {
        Column::pg_type(*self)
    }

    fn pg_default(&self) -> Option<&'static str> {
        Column::pg_default(*self)
    }

    fn nullable(&self) -> bool {
        Column::nullable(*self)
    }

    fn query_ops(&self) -> &'static [FilterOperator] {
        Column::query_ops(*self)
    }
}

/// Columns selected in the caller's order; an unknown name fails to deserialize, so no request string reaches SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Columns<C: Column>(IndexSet<C>);

impl<C: Column> PartialEq for Columns<C> {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().eq(other.0.iter())
    }
}

impl<C: Column> Eq for Columns<C> {}

impl<C: Column> Columns<C> {
    /// Every column of the model.
    pub fn all() -> Self {
        Self(C::all().iter().copied().collect())
    }

    pub fn insert(&mut self, column: C) -> bool {
        self.0.insert(column)
    }

    /// Removes a column, keeping the order of the rest.
    pub fn remove(&mut self, column: C) -> bool {
        self.0.shift_remove(&column)
    }

    pub fn contains(&self, column: C) -> bool {
        self.0.contains(&column)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The columns in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = C> + '_ {
        self.0.iter().copied()
    }

    /// The column names in insertion order, ready for a `SELECT` list.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.iter().map(|column| Column::name(*column))
    }

    /// The column types in insertion order, as DDL spells them.
    pub fn pg_types(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.iter().map(|column| Column::pg_type(*column))
    }
}

impl<C: Column> Default for Columns<C> {
    fn default() -> Self {
        Self(IndexSet::new())
    }
}

impl<C: Column> FromIterator<C> for Columns<C> {
    fn from_iter<I: IntoIterator<Item = C>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<C: Column> IntoIterator for Columns<C> {
    type IntoIter = indexmap::set::IntoIter<C>;
    type Item = C;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
