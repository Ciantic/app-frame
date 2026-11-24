use serde::Deserialize;
use serde::Serialize;
use serde::de::Visitor;

/// A comparison a column supports, as declared by
/// `#[db(query_ops(...))]`.
///
/// The name is the annotation name, the JSON key suffix and the method name on
/// a column handle, so there is one vocabulary to learn. An operator that takes
/// no value, [`IsNull`](Self::IsNull) and [`IsNotNull`](Self::IsNotNull), needs
/// no declaration: it is always available and cannot be mistyped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FilterOperator {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    ILike,
    NotLike,
    NotILike,
    In,
    NotIn,
    /// Whether an array column holds the value: `$1 = ANY("column")`.
    Contains,
    NotContains,
    /// Whether an array column shares any element with the list.
    Overlaps,
    NotOverlaps,
    /// Whether an array column holds every element of the list.
    ContainsAll,
    NotContainsAll,
    IsNull,
    IsNotNull,
    Between,
    NotBetween,
}

impl FilterOperator {
    /// Every operator, in the order the vocabulary lists them.
    pub const ALL: &'static [FilterOperator] = &[
        FilterOperator::Eq,
        FilterOperator::Neq,
        FilterOperator::Gt,
        FilterOperator::Gte,
        FilterOperator::Lt,
        FilterOperator::Lte,
        FilterOperator::Like,
        FilterOperator::ILike,
        FilterOperator::NotLike,
        FilterOperator::NotILike,
        FilterOperator::In,
        FilterOperator::NotIn,
        FilterOperator::Contains,
        FilterOperator::NotContains,
        FilterOperator::Overlaps,
        FilterOperator::NotOverlaps,
        FilterOperator::ContainsAll,
        FilterOperator::NotContainsAll,
        FilterOperator::IsNull,
        FilterOperator::IsNotNull,
        FilterOperator::Between,
        FilterOperator::NotBetween,
    ];

    /// The operator as the annotation, the JSON key suffix and the handle
    /// method spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            FilterOperator::Eq => "eq",
            FilterOperator::Neq => "neq",
            FilterOperator::Gt => "gt",
            FilterOperator::Gte => "gte",
            FilterOperator::Lt => "lt",
            FilterOperator::Lte => "lte",
            FilterOperator::Like => "like",
            FilterOperator::ILike => "ilike",
            FilterOperator::NotLike => "not_like",
            FilterOperator::NotILike => "not_ilike",
            FilterOperator::In => "in",
            FilterOperator::NotIn => "not_in",
            FilterOperator::Contains => "contains",
            FilterOperator::NotContains => "not_contains",
            FilterOperator::Overlaps => "overlaps",
            FilterOperator::NotOverlaps => "not_overlaps",
            FilterOperator::ContainsAll => "contains_all",
            FilterOperator::NotContainsAll => "not_contains_all",
            FilterOperator::IsNull => "is_null",
            FilterOperator::IsNotNull => "is_not_null",
            FilterOperator::Between => "between",
            FilterOperator::NotBetween => "not_between",
        }
    }

    /// The operator of that name, or `None` when no operator is spelled so.
    pub fn from_name(name: &str) -> Option<Self> {
        FilterOperator::ALL
            .iter()
            .copied()
            .find(|operator| operator.as_str() == name)
    }

    /// Whether the operator takes no value, so it needs no `query_ops`
    /// declaration and no operand.
    pub fn is_nullary(self) -> bool {
        matches!(self, FilterOperator::IsNull | FilterOperator::IsNotNull)
    }

    /// The names of every operator, for an error that lists what is accepted.
    pub fn names() -> impl Iterator<Item = &'static str> {
        FilterOperator::ALL.iter().map(|operator| operator.as_str())
    }
}

impl Serialize for FilterOperator {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FilterOperator {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct OperatorVisitor;

        impl Visitor<'_> for OperatorVisitor {
            type Value = FilterOperator;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a filter operator name")
            }

            fn visit_str<E: serde::de::Error>(
                self,
                name: &str,
            ) -> std::result::Result<FilterOperator, E> {
                FilterOperator::from_name(name).ok_or_else(|| {
                    E::custom(format_args!(
                        "operator `{name}` is not known; expected one of: {}",
                        FilterOperator::names().collect::<Vec<_>>().join(", "),
                    ))
                })
            }
        }

        deserializer.deserialize_str(OperatorVisitor)
    }
}

/// A column of a model table, erased so that one table's description can hold
/// columns from several models.
///
/// The generated `<Model>Column` enums implement [`crate::Column`] and describe
/// their columns; the blanket implementation forwards here. A `#[db(flatten)]`
/// field splices another type's `TableMeta::columns`, which is why the slice
/// holds trait objects rather than one enum.
pub trait ErasedColumn: std::fmt::Debug + Send + Sync + 'static {
    /// The column name as the database spells it.
    fn name(&self) -> &'static str;

    /// The column type as DDL spells it: `uuid`, `text`, `varchar(255)`.
    fn pg_type(&self) -> &'static str;

    /// The column default as DDL spells it, or `None` when the column has none.
    fn pg_default(&self) -> Option<&'static str>;

    /// Whether the column is nullable.
    fn nullable(&self) -> bool;

    /// Operators the column may be queried with, in declaration order.
    fn query_ops(&self) -> &'static [FilterOperator];
}

/// Columns compare by name: a table cannot declare one name twice, and a
/// spliced fragment keeps its names.
impl PartialEq for dyn ErasedColumn {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for dyn ErasedColumn {}

/// Table-level description generated by `#[derive(DbModel)]` on a
/// `#[db(table = "...")]` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableMeta {
    pub table: &'static str,
    /// Primary key columns in declaration order; never empty.
    pub primary_key: &'static [&'static str],
    pub version: Option<&'static str>,
    pub created_at: Option<&'static str>,
    pub updated_at: Option<&'static str>,
    /// Every column in declaration order, with the columns of
    /// `#[db(flatten)]` fields spliced in place.
    pub columns: &'static [&'static dyn ErasedColumn],
}

/// Description generated by `#[derive(DbModel)]` on a `#[db(fragment)]`
/// struct: a group of columns that another model splices in with
/// `#[db(flatten)]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableFragment {
    /// Every column in declaration order.
    pub columns: &'static [&'static dyn ErasedColumn],
}

/// What a model describes, generated by `#[derive(DbModel)]` and returned by
/// [`Model::meta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meta {
    Table(&'static TableMeta),
    Fragment(&'static TableFragment),
}

impl Meta {
    /// Columns in declaration order, whether the model is a table or a
    /// fragment.
    pub fn columns(self) -> &'static [&'static dyn ErasedColumn] {
        match self {
            Meta::Table(meta) => meta.columns,
            Meta::Fragment(fragment) => fragment.columns,
        }
    }

    /// The column of that name, or `None` when the model has no such column.
    pub fn column(self, name: &str) -> Option<&'static dyn ErasedColumn> {
        self.columns()
            .iter()
            .copied()
            .find(|column| column.name() == name)
    }

    /// The table description, or `None` for a fragment.
    pub fn table(self) -> Option<&'static TableMeta> {
        match self {
            Meta::Table(meta) => Some(meta),
            Meta::Fragment(_) => None,
        }
    }

    /// The owned table description, which a crate's `MODELS` list collects.
    ///
    /// # Panics
    ///
    /// Panics when the model is a fragment, because a fragment is not a table
    /// and must not be listed in `MODELS`.
    pub fn into_table(self) -> TableMeta {
        match self {
            Meta::Table(meta) => *meta,
            Meta::Fragment(_) => {
                panic!("a `#[db(fragment)]` struct is not a table and cannot be listed in `MODELS`")
            }
        }
    }
}

/// The database column type of a value type, as declared by
/// `#[derive(DbType)]` on its definition.
///
/// A model field whose type implements this trait may omit `pgtype` and
/// `pgdefault`, taking both from the type definition instead.
pub trait DbType {
    /// Column type as DDL spells it: `uuid`, `text`, `varchar(255)`.
    const PG_TYPE: &'static str;

    /// Column default as DDL spells it, or `None` when the type has none.
    const PG_DEFAULT: Option<&'static str> = None;
}

/// A model whose projected rows can be decoded.
///
/// `#[derive(DbSelect)]` generates `{Model}Select`, one `Selected<T>` field per
/// column, and implements this trait to name it. [`Query::build`] is bounded on
/// it, so a model without the derive can still render SQL but cannot be turned
/// into a query that decodes rows.
///
/// [`Query::build`]: crate::Query::build
pub trait Selectable: Model {
    /// The struct a projected row decodes into.
    type Select: crate::SelectRow + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin;
}

/// A row type of one table, or a group of columns embedded in one.
pub trait Model {
    /// What the model describes, built on first use and cached.
    fn meta() -> Meta;
}
