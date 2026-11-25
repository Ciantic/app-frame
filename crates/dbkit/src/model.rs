use serde::Deserialize;
use serde::Serialize;
use serde::de::Visitor;

/// A comparison a column supports, declared with `#[db(query_ops(...))]`; one name for annotation, JSON and method.
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
    /// `"column" = ANY($n)`, the list bound as one array parameter.
    Any,
    /// `"column" <> ALL($n)`.
    NotAny,
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
        FilterOperator::Any,
        FilterOperator::NotAny,
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

    /// The operator as the annotation, the JSON key suffix and the handle method spell it.
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
            FilterOperator::Any => "any",
            FilterOperator::NotAny => "not_any",
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

    /// Whether the operator takes no value, so it needs no `query_ops` declaration and no operand.
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

/// A column erased so one model's metadata can hold columns spliced in from another; `<Model>Column` forwards here.
pub trait ErasedColumn: std::fmt::Debug + Send + Sync + 'static {
    /// The column name as the database spells it.
    fn name(&self) -> &'static str;

    /// Whether the column may appear in `ORDER BY`, as declared by `#[db(orderable)]`.
    fn orderable(&self) -> bool;

    /// The column type as DDL spells it: `uuid`, `text`, `varchar(255)`.
    fn pg_type(&self) -> &'static str;

    /// The column default as DDL spells it, or `None` when the column has none.
    fn pg_default(&self) -> Option<&'static str>;

    /// Whether the column is nullable.
    fn nullable(&self) -> bool;

    /// The generated-column expression, or `None` when the column is ordinary.
    fn generated(&self) -> Option<&'static str>;

    /// Operators the column may be queried with, in declaration order.
    fn query_ops(&self) -> &'static [FilterOperator];

    /// Triggers that fill this column, in declaration order.
    fn triggers(&self) -> &'static [TriggerRef];
}

/// Columns compare by name: a table cannot declare one name twice, and a spliced fragment keeps its names.
impl PartialEq for dyn ErasedColumn {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for dyn ErasedColumn {}

/// The cardinality of a `#[db(relation)]` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// The field holds one related row, typically as `Option<T>`.
    One,
    /// The field holds many related rows, typically as `Vec<T>`.
    Many,
}

/// A related field declared with `#[db(relation)]`; it has no column of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relation {
    pub field: &'static str,
    pub model: &'static str,
    pub table: &'static str,
    pub cardinality: Cardinality,
    pub local_key: &'static [&'static str],
    pub target_key: &'static [&'static str],
}

/// A foreign key declared with `#[db(foreign_key(...))]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignKey {
    pub columns: &'static [&'static str],
    pub table: &'static str,
    pub referenced_columns: &'static [&'static str],
}

/// A unique key declared with `#[db(unique)]` or `#[db(unique(columns = [...]))]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniqueKey {
    pub columns: &'static [&'static str],
}

/// When a trigger fires, declared with `timing = before` or `timing = after`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTiming {
    Before,
    After,
}

impl TriggerTiming {
    /// The keyword as DDL spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerTiming::Before => "before",
            TriggerTiming::After => "after",
        }
    }
}

/// Whether a trigger fires once per row or once per statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerLevel {
    Row,
    Statement,
}

impl TriggerLevel {
    /// The keyword as DDL spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerLevel::Row => "row",
            TriggerLevel::Statement => "statement",
        }
    }
}

/// A statement that fires a trigger, declared inside `events(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TriggerEvent {
    Insert,
    Update,
    Delete,
    Truncate,
}

impl TriggerEvent {
    /// Every event, in the canonical order the derive records them.
    pub const ALL: &'static [TriggerEvent] = &[
        TriggerEvent::Insert,
        TriggerEvent::Update,
        TriggerEvent::Delete,
        TriggerEvent::Truncate,
    ];

    /// The keyword as DDL spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerEvent::Insert => "insert",
            TriggerEvent::Update => "update",
            TriggerEvent::Delete => "delete",
            TriggerEvent::Truncate => "truncate",
        }
    }
}

/// A trigger declared by a model, and the table it is installed on; that table may be another model's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerRef {
    pub name: &'static str,
    /// The table the trigger is installed on, from `on = "..."` or the declaring model's table.
    pub table: &'static str,
}

/// The full definition of a trigger, from `#[db(pg_trigger(...))]` with a `body`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigger {
    pub name: &'static str,
    pub table: &'static str,
    pub timing: TriggerTiming,
    pub events: &'static [TriggerEvent],
    pub level: TriggerLevel,
    /// The optional `WHEN` condition, as DDL spells it.
    pub condition: Option<&'static str>,
    /// The function name, defaulting to the trigger name when `function` is omitted.
    pub function: &'static str,
    /// The function's language, defaulting to `plpgsql`.
    pub language: &'static str,
    /// The function body exactly as the annotation spells it.
    pub body: &'static str,
}

/// Table-level description generated by `#[derive(DbModel)]` on a `#[db(table = "...")]` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableMeta {
    pub table: &'static str,
    /// Primary key columns in declaration order; never empty.
    pub primary_key: &'static [&'static str],
    pub version: Option<&'static str>,
    pub created_at: Option<&'static str>,
    pub updated_at: Option<&'static str>,
    /// The tenant ownership column, when the table declares `#[db(tenant)]`.
    pub tenant: Option<&'static str>,
    /// Every column in declaration order, with the columns of `#[db(flatten)]` fields spliced in place.
    pub columns: &'static [&'static dyn ErasedColumn],
    /// Related fields declared with `#[db(relation)]`, in declaration order.
    pub relations: &'static [Relation],
    /// Foreign keys declared with `#[db(foreign_key)]`, in declaration order.
    pub foreign_keys: &'static [ForeignKey],
    /// Unique keys declared with `#[db(unique)]`, in declaration order.
    pub unique_keys: &'static [UniqueKey],
    /// Every trigger declaration in declaration order, including triggers installed on another table.
    pub triggers: &'static [TriggerRef],
    /// Full trigger definitions from `#[db(pg_trigger(...))]` with a `body`, in declaration order.
    pub trigger_definitions: &'static [Trigger],
}

/// A fragment's description: a group of columns another model splices in with `#[db(flatten)]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableFragment {
    /// Every column in declaration order.
    pub columns: &'static [&'static dyn ErasedColumn],
}

/// A subquery model's description: read-only rows rendered as `SELECT ... FROM (sql) AS "subquery"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubQuery {
    /// The SQL of the derived table, as the annotation spells it.
    pub sql: &'static str,
    /// Every column the query returns, in declaration order.
    pub columns: &'static [&'static dyn ErasedColumn],
}

/// What a model describes, generated by `#[derive(DbModel)]` and returned by [`Model::meta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meta {
    Table(&'static TableMeta),
    SubQuery(&'static SubQuery),
    Fragment(&'static TableFragment),
}

impl Meta {
    /// Columns in declaration order, whatever the model describes.
    pub fn columns(self) -> &'static [&'static dyn ErasedColumn] {
        match self {
            Meta::Table(meta) => meta.columns,
            Meta::SubQuery(subquery) => subquery.columns,
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

    /// The table description, or `None` when the model is a subquery or a fragment.
    pub fn table(self) -> Option<&'static TableMeta> {
        match self {
            Meta::Table(meta) => Some(meta),
            Meta::SubQuery(_) | Meta::Fragment(_) => None,
        }
    }

    /// The table's tenant ownership column, when it declares `#[db(tenant)]`.
    pub fn tenant(self) -> Option<&'static str> {
        self.table().and_then(|table| table.tenant)
    }

    /// The subquery description, or `None` when the model is a table or a fragment.
    pub fn subquery(self) -> Option<&'static SubQuery> {
        match self {
            Meta::SubQuery(subquery) => Some(subquery),
            Meta::Table(_) | Meta::Fragment(_) => None,
        }
    }

    /// Owned table description for `MODELS`; panics when the model is a subquery or a fragment.
    pub fn into_table(self) -> TableMeta {
        match self {
            Meta::Table(meta) => *meta,
            Meta::SubQuery(_) => panic!(
                "a `#[db(subquery = \"...\")]` struct is not a table and cannot be listed in \
                 `MODELS`"
            ),
            Meta::Fragment(_) => {
                panic!("a `#[db(fragment)]` struct is not a table and cannot be listed in `MODELS`")
            }
        }
    }
}

/// The database column type of a value type; a field of that type may omit `pgtype` and `pgdefault`.
pub trait DbType {
    /// Column type as DDL spells it: `uuid`, `text`, `varchar(255)`.
    const PG_TYPE: &'static str;

    /// Column default as DDL spells it, or `None` when the type has none.
    const PG_DEFAULT: Option<&'static str> = None;
}

/// A model whose projections decode through `#[derive(DbSelect)]`'s `{Model}Select`, which `build_selected` needs.
pub trait Selectable: Model {
    /// The struct a projected row decodes into.
    type Select: crate::SelectRow + for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin;
}

/// A row type of one table, the rows one query produces, or a group of columns embedded in a table or a subquery.
pub trait Model {
    /// What the model describes, built on first use and cached.
    fn meta() -> Meta;
}

/// A table whose rows belong to a tenant, declared with `#[db(tenant)]` on the ownership column.
pub trait TenantOwned: Model {
    /// The tenant ownership column as the database spells it.
    const TENANT_COLUMN: &'static str;
}

/// The tenant value an insert, patch or key carries; `None` means the model is not tenant-owned.
pub trait TenantRow {
    fn tenant(&self) -> Option<&dyn crate::query::Bind>;
}
