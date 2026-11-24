//! The filter tree, its operand, and the typed handle a column is filtered
//! through.

use crate::Column;
use crate::Columns;
use crate::FilterOperator;
use crate::query::Bind;
use crate::query::capability::HasBetween;
use crate::query::capability::HasContains;
use crate::query::capability::HasContainsAll;
use crate::query::capability::HasEq;
use crate::query::capability::HasGt;
use crate::query::capability::HasGte;
use crate::query::capability::HasILike;
use crate::query::capability::HasIn;
use crate::query::capability::HasLike;
use crate::query::capability::HasLt;
use crate::query::capability::HasLte;
use crate::query::capability::HasNeq;
use crate::query::capability::HasNotBetween;
use crate::query::capability::HasNotContains;
use crate::query::capability::HasNotContainsAll;
use crate::query::capability::HasNotILike;
use crate::query::capability::HasNotIn;
use crate::query::capability::HasNotLike;
use crate::query::capability::HasNotOverlaps;
use crate::query::capability::HasOverlaps;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

/// One comparison of a column, or a group of filters.
///
/// A leaf handle returns `Filter`, not `Leaf`, so one array can mix leaves and
/// groups: a `Vec` holds one element type, and the enum is what allows the mix.
#[derive(Clone)]
pub enum Filter<C: Column> {
    Leaf(Leaf<C>),
    Group(Group<C>),
}

/// One comparison: a column, the operator, and its operand.
#[derive(Clone)]
pub struct Leaf<C: Column> {
    pub column: C,
    pub operator: FilterOperator,
    pub operand: Operand,
}

/// What a leaf binds.
///
/// The arity is in the type so a hand-built tree cannot silently render two
/// placeholders for one value; [`build`](crate::Query::build) still checks that
/// the operator agrees with it.
#[derive(Clone)]
pub enum Operand {
    /// A single value.
    One(Arc<dyn Bind>),
    /// Two values, for `between`.
    Two(Arc<dyn Bind>, Arc<dyn Bind>),
    /// One array value, bound as one parameter, and its element count.
    List { values: Arc<dyn Bind>, len: usize },
    /// No value, for `is_null`, `is_not_null` and `neq` with `null`.
    None,
}

impl Operand {
    pub fn one(value: impl Bind) -> Self {
        Operand::One(Arc::new(value))
    }

    pub fn two(low: impl Bind, high: impl Bind) -> Self {
        Operand::Two(Arc::new(low), Arc::new(high))
    }

    pub fn list<T: Bind>(values: Vec<T>) -> Self
    where
        Vec<T>: Bind,
    {
        Operand::List {
            len: values.len(),
            values: Arc::new(values),
        }
    }

    /// The single value, for a leaf that binds one.
    pub fn value(&self) -> Option<&dyn Bind> {
        match self {
            Operand::One(value) => Some(&**value),
            _ => None,
        }
    }

    /// What the operand holds, for an error that says what arrived.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Operand::One(_) => "one value",
            Operand::Two(_, _) => "two values",
            Operand::List { .. } => "a list",
            Operand::None => "no value",
        }
    }

    fn eq_operand(&self, other: &Self) -> bool {
        match (self, other) {
            (Operand::One(left), Operand::One(right)) => left.eq_dyn(&**right),
            (Operand::Two(left_low, left_high), Operand::Two(right_low, right_high)) => {
                left_low.eq_dyn(&**right_low) && left_high.eq_dyn(&**right_high)
            }
            (
                Operand::List {
                    values: left,
                    len: left_len,
                },
                Operand::List {
                    values: right,
                    len: right_len,
                },
            ) => left_len == right_len && left.eq_dyn(&**right),
            (Operand::None, Operand::None) => true,
            _ => false,
        }
    }
}

/// How the children of a group combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Combinator {
    /// Children must all hold; the top level is this one.
    #[default]
    And,
    /// Any child may hold.
    Or,
}

impl Combinator {
    /// The word the SQL and the JSON key both use.
    pub fn as_str(self) -> &'static str {
        match self {
            Combinator::And => "and",
            Combinator::Or => "or",
        }
    }
}

/// A combinator and its children. Every group renders in parentheses, so SQL
/// precedence never decides the meaning.
#[derive(Clone)]
pub struct Group<C: Column> {
    pub combinator: Combinator,
    pub filters: Vec<Filter<C>>,
}

impl<C: Column> Filter<C> {
    /// A comparison, with the operand the operator needs.
    ///
    /// This constructor is untyped on purpose: a handle exists for every
    /// operator a column declares, and this is the way to build one it does
    /// not cover. `build()` validates it like any other leaf.
    pub fn leaf(column: C, operator: FilterOperator, operand: Operand) -> Self {
        Filter::Leaf(Leaf {
            column,
            operator,
            operand,
        })
    }

    pub fn group(combinator: Combinator, filters: impl IntoIterator<Item = Filter<C>>) -> Self {
        Filter::Group(Group {
            combinator,
            filters: filters.into_iter().collect(),
        })
    }

    pub fn and(filters: impl IntoIterator<Item = Filter<C>>) -> Self {
        Filter::group(Combinator::And, filters)
    }

    pub fn or(filters: impl IntoIterator<Item = Filter<C>>) -> Self {
        Filter::group(Combinator::Or, filters)
    }

    /// Flattens a same-combinator child into its parent and drops a group with
    /// one child, recursively. Opt-in, because it rewrites the tree and so
    /// changes what serialization prints.
    pub fn normalize(self) -> Self {
        let Filter::Group(group) = self else {
            return self;
        };

        let mut filters = Vec::with_capacity(group.filters.len());
        for filter in group.filters {
            match filter.normalize() {
                Filter::Group(child) if child.combinator == group.combinator => {
                    filters.extend(child.filters);
                }
                other => filters.push(other),
            }
        }

        match filters.len() {
            1 => filters.pop().expect("one filter"),
            _ => Filter::Group(Group {
                combinator: group.combinator,
                filters,
            }),
        }
    }
}

impl<C: Column> fmt::Debug for Filter<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Filter::Leaf(leaf) => formatter
                .debug_struct("Leaf")
                .field("column", &leaf.column.name())
                .field("operator", &leaf.operator)
                .field("operand", &leaf.operand)
                .finish(),
            Filter::Group(group) => formatter
                .debug_struct("Group")
                .field("combinator", &group.combinator)
                .field("filters", &group.filters)
                .finish(),
        }
    }
}

impl fmt::Debug for Operand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::One(value) => formatter.debug_tuple("One").field(value).finish(),
            Operand::Two(low, high) => formatter.debug_tuple("Two").field(low).field(high).finish(),
            Operand::List { values, len } => formatter
                .debug_struct("List")
                .field("values", values)
                .field("len", len)
                .finish(),
            Operand::None => formatter.write_str("None"),
        }
    }
}

impl<C: Column> PartialEq for Filter<C> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Filter::Leaf(left), Filter::Leaf(right)) => {
                left.column == right.column
                    && left.operator == right.operator
                    && left.operand.eq_operand(&right.operand)
            }
            (Filter::Group(left), Filter::Group(right)) => {
                left.combinator == right.combinator && left.filters == right.filters
            }
            _ => false,
        }
    }
}

impl<C: Column> Eq for Filter<C> {}

/// A column with its value type and the tag that says which operators it
/// declared.
///
/// The column enum cannot carry a per-variant value type, because an associated
/// type belongs to an impl rather than to a variant. `Order::columns()` builds
/// one of these per field, and `#[derive(DbModel)]` emits one empty tag type
/// per column with one capability impl per `query_ops` entry, so an operator
/// the column never declared does not resolve.
pub struct ColumnRef<Tag, C: Column, T> {
    column: C,
    tag: PhantomData<fn() -> Tag>,
    value: PhantomData<fn() -> T>,
}

// Manual, so `Copy` does not require `Tag: Copy` or `T: Copy`.
impl<Tag, C: Column, T> Copy for ColumnRef<Tag, C, T> {}

impl<Tag, C: Column, T> Clone for ColumnRef<Tag, C, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, C: Column, T> fmt::Debug for ColumnRef<Tag, C, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ColumnRef")
            .field(&self.column.name())
            .finish()
    }
}

// No `PartialEq`: its `eq` would resolve when a column's `eq` is not declared,
// which would turn "operator not declared" into a type mismatch.

impl<Tag, C: Column, T> ColumnRef<Tag, C, T> {
    /// The handle of a column, for the generated `columns()`.
    #[doc(hidden)]
    pub const fn new(column: C) -> Self {
        Self {
            column,
            tag: PhantomData,
            value: PhantomData,
        }
    }

    /// The column the handle names.
    pub const fn column(self) -> C {
        self.column
    }

    /// `IS NULL`. Available on every column: on a `NOT NULL` one it simply
    /// matches nothing.
    pub fn is_null(self) -> Filter<C> {
        Filter::leaf(self.column, FilterOperator::IsNull, Operand::None)
    }

    pub fn is_not_null(self) -> Filter<C> {
        Filter::leaf(self.column, FilterOperator::IsNotNull, Operand::None)
    }

    fn leaf(self, operator: FilterOperator, operand: Operand) -> Filter<C> {
        Filter::leaf(self.column, operator, operand)
    }
}

impl<Tag: HasEq<C>, C: Column, T: Bind> ColumnRef<Tag, C, T> {
    pub fn eq(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Eq, Operand::one(value.into()))
    }
}

impl<Tag: HasNeq<C>, C: Column, T: Bind> ColumnRef<Tag, C, T> {
    pub fn neq(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Neq, Operand::one(value.into()))
    }
}

impl<Tag: HasGt<C>, C: Column, T: Bind + PartialOrd> ColumnRef<Tag, C, T> {
    pub fn gt(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Gt, Operand::one(value.into()))
    }
}

impl<Tag: HasGte<C>, C: Column, T: Bind + PartialOrd> ColumnRef<Tag, C, T> {
    pub fn gte(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Gte, Operand::one(value.into()))
    }
}

impl<Tag: HasLt<C>, C: Column, T: Bind + PartialOrd> ColumnRef<Tag, C, T> {
    pub fn lt(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Lt, Operand::one(value.into()))
    }
}

impl<Tag: HasLte<C>, C: Column, T: Bind + PartialOrd> ColumnRef<Tag, C, T> {
    pub fn lte(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Lte, Operand::one(value.into()))
    }
}

impl<Tag: HasBetween<C>, C: Column, T: Bind + PartialOrd> ColumnRef<Tag, C, T> {
    pub fn between(self, low: impl Into<T>, high: impl Into<T>) -> Filter<C> {
        self.leaf(
            FilterOperator::Between,
            Operand::two(low.into(), high.into()),
        )
    }
}

impl<Tag: HasNotBetween<C>, C: Column, T: Bind + PartialOrd> ColumnRef<Tag, C, T> {
    pub fn not_between(self, low: impl Into<T>, high: impl Into<T>) -> Filter<C> {
        self.leaf(
            FilterOperator::NotBetween,
            Operand::two(low.into(), high.into()),
        )
    }
}

impl<Tag: HasIn<C>, C: Column, T: Bind> ColumnRef<Tag, C, T> {
    pub fn in_<I>(self, values: I) -> Filter<C>
    where
        I: IntoIterator,
        I::Item: Into<T>,
        Vec<T>: Bind,
    {
        let values: Vec<T> = values.into_iter().map(Into::into).collect();

        self.leaf(FilterOperator::In, Operand::list(values))
    }
}

impl<Tag: HasNotIn<C>, C: Column, T: Bind> ColumnRef<Tag, C, T> {
    pub fn not_in<I>(self, values: I) -> Filter<C>
    where
        I: IntoIterator,
        I::Item: Into<T>,
        Vec<T>: Bind,
    {
        let values: Vec<T> = values.into_iter().map(Into::into).collect();

        self.leaf(FilterOperator::NotIn, Operand::list(values))
    }
}

impl<Tag: HasLike<C>, C: Column> ColumnRef<Tag, C, String> {
    pub fn like(self, pattern: impl Into<String>) -> Filter<C> {
        self.leaf(FilterOperator::Like, Operand::one(pattern.into()))
    }
}

impl<Tag: HasILike<C>, C: Column> ColumnRef<Tag, C, String> {
    pub fn ilike(self, pattern: impl Into<String>) -> Filter<C> {
        self.leaf(FilterOperator::ILike, Operand::one(pattern.into()))
    }
}

impl<Tag: HasNotLike<C>, C: Column> ColumnRef<Tag, C, String> {
    pub fn not_like(self, pattern: impl Into<String>) -> Filter<C> {
        self.leaf(FilterOperator::NotLike, Operand::one(pattern.into()))
    }
}

impl<Tag: HasNotILike<C>, C: Column> ColumnRef<Tag, C, String> {
    pub fn not_ilike(self, pattern: impl Into<String>) -> Filter<C> {
        self.leaf(FilterOperator::NotILike, Operand::one(pattern.into()))
    }
}

impl<Tag: HasContains<C>, C: Column, T: Bind> ColumnRef<Tag, C, Vec<T>> {
    pub fn contains(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::Contains, Operand::one(value.into()))
    }
}

impl<Tag: HasNotContains<C>, C: Column, T: Bind> ColumnRef<Tag, C, Vec<T>> {
    pub fn not_contains(self, value: impl Into<T>) -> Filter<C> {
        self.leaf(FilterOperator::NotContains, Operand::one(value.into()))
    }
}

impl<Tag: HasOverlaps<C>, C: Column, T: Bind> ColumnRef<Tag, C, Vec<T>>
where
    Vec<T>: Bind,
{
    pub fn overlaps<I>(self, values: I) -> Filter<C>
    where
        I: IntoIterator,
        I::Item: Into<T>,
    {
        let values: Vec<T> = values.into_iter().map(Into::into).collect();

        self.leaf(FilterOperator::Overlaps, Operand::list(values))
    }
}

impl<Tag: HasNotOverlaps<C>, C: Column, T: Bind> ColumnRef<Tag, C, Vec<T>>
where
    Vec<T>: Bind,
{
    pub fn not_overlaps<I>(self, values: I) -> Filter<C>
    where
        I: IntoIterator,
        I::Item: Into<T>,
    {
        let values: Vec<T> = values.into_iter().map(Into::into).collect();

        self.leaf(FilterOperator::NotOverlaps, Operand::list(values))
    }
}

impl<Tag: HasContainsAll<C>, C: Column, T: Bind> ColumnRef<Tag, C, Vec<T>>
where
    Vec<T>: Bind,
{
    pub fn contains_all<I>(self, values: I) -> Filter<C>
    where
        I: IntoIterator,
        I::Item: Into<T>,
    {
        let values: Vec<T> = values.into_iter().map(Into::into).collect();

        self.leaf(FilterOperator::ContainsAll, Operand::list(values))
    }
}

impl<Tag: HasNotContainsAll<C>, C: Column, T: Bind> ColumnRef<Tag, C, Vec<T>>
where
    Vec<T>: Bind,
{
    pub fn not_contains_all<I>(self, values: I) -> Filter<C>
    where
        I: IntoIterator,
        I::Item: Into<T>,
    {
        let values: Vec<T> = values.into_iter().map(Into::into).collect();

        self.leaf(FilterOperator::NotContainsAll, Operand::list(values))
    }
}

/// What a selection may be built from: one column, one handle, a prebuilt set,
/// or a list of columns, owned or borrowed.
///
/// The element types differ, because a handle is one type per column, so the
/// trait is what lets `select` take a mix of them. Only the array form needs a
/// single element type, and a handle reduces to its column with
/// [`column()`](ColumnRef::column).
pub trait IntoColumns<C: Column> {
    fn into_columns(self) -> Columns<C>;
}

impl<C: Column> IntoColumns<C> for C {
    fn into_columns(self) -> Columns<C> {
        [self].into_iter().collect()
    }
}

impl<Tag, C: Column, T> IntoColumns<C> for ColumnRef<Tag, C, T> {
    fn into_columns(self) -> Columns<C> {
        [self.column].into_iter().collect()
    }
}

impl<C: Column> IntoColumns<C> for Columns<C> {
    fn into_columns(self) -> Columns<C> {
        self
    }
}

impl<C: Column, const N: usize> IntoColumns<C> for [C; N] {
    fn into_columns(self) -> Columns<C> {
        self.into_iter().collect()
    }
}

// Through a reference, so a caller can pass a list it does not own. These do
// not overlap the impls above: `Column` is this crate's trait, and none of
// these reference types implements it.
impl<C: Column, const N: usize> IntoColumns<C> for &[C; N] {
    fn into_columns(self) -> Columns<C> {
        self.iter().copied().collect()
    }
}

impl<C: Column> IntoColumns<C> for &[C] {
    fn into_columns(self) -> Columns<C> {
        self.iter().copied().collect()
    }
}

impl<C: Column> IntoColumns<C> for Vec<C> {
    fn into_columns(self) -> Columns<C> {
        self.into_iter().collect()
    }
}

impl<C: Column> IntoColumns<C> for &Vec<C> {
    fn into_columns(self) -> Columns<C> {
        self.iter().copied().collect()
    }
}

impl<C: Column> IntoColumns<C> for &Columns<C> {
    fn into_columns(self) -> Columns<C> {
        self.iter().collect()
    }
}
