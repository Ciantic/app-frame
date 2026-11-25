//! Parsing and validation of the `#[db(...)]` annotations.

use quote::ToTokens;
use syn::Attribute;
use syn::Data;
use syn::DeriveInput;
use syn::Field;
use syn::Fields;
use syn::Ident;
use syn::LitStr;
use syn::Type;
use syn::spanned::Spanned;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    PrimaryKey,
    Version,
    CreatedAt,
    UpdatedAt,
    Tenant,
}

/// A `dbkit::FilterOperator` variant, known to the macro by its annotation name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    Any,
    NotAny,
    Contains,
    NotContains,
    Overlaps,
    NotOverlaps,
    ContainsAll,
    NotContainsAll,
    IsNull,
    IsNotNull,
    Between,
    NotBetween,
}

impl FilterOperator {
    /// The `dbkit::FilterOperator` variant this operator emits.
    pub fn variant(self) -> &'static str {
        match self {
            FilterOperator::Eq => "Eq",
            FilterOperator::Neq => "Neq",
            FilterOperator::Gt => "Gt",
            FilterOperator::Gte => "Gte",
            FilterOperator::Lt => "Lt",
            FilterOperator::Lte => "Lte",
            FilterOperator::Like => "Like",
            FilterOperator::ILike => "ILike",
            FilterOperator::NotLike => "NotLike",
            FilterOperator::NotILike => "NotILike",
            FilterOperator::Any => "Any",
            FilterOperator::NotAny => "NotAny",
            FilterOperator::Contains => "Contains",
            FilterOperator::NotContains => "NotContains",
            FilterOperator::Overlaps => "Overlaps",
            FilterOperator::NotOverlaps => "NotOverlaps",
            FilterOperator::ContainsAll => "ContainsAll",
            FilterOperator::NotContainsAll => "NotContainsAll",
            FilterOperator::IsNull => "IsNull",
            FilterOperator::IsNotNull => "IsNotNull",
            FilterOperator::Between => "Between",
            FilterOperator::NotBetween => "NotBetween",
        }
    }
}

/// What the struct describes: a table row, a query's rows, or columns another model embeds with `#[db(flatten)]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Table(String),
    SubQuery(String),
    Fragment,
}

#[derive(Debug)]
pub struct Annotations {
    pub shape: Shape,
    /// Primary key columns in declaration order; never empty for a table.
    pub primary_key: Vec<Ident>,
    pub version: Option<Ident>,
    pub created_at: Option<Ident>,
    pub updated_at: Option<Ident>,
    pub tenant: Option<Ident>,
    /// Every column in declaration order.
    pub columns: Vec<Column>,
    /// Related fields in declaration order, declared with `#[db(relation)]`.
    pub relations: Vec<RelationAnnotation>,
    /// Foreign keys in declaration order, declared with `#[db(foreign_key)]`.
    pub foreign_keys: Vec<ResolvedForeignKey>,
    /// Trigger declarations in declaration order, from `#[db(pg_trigger)]`.
    pub triggers: Vec<TriggerRef>,
    /// Full trigger definitions in declaration order, from `#[db(pg_trigger(...))]` with a `body`.
    pub trigger_definitions: Vec<TriggerDefinition>,
    /// Unique keys in declaration order, from `#[db(unique)]`.
    pub unique_keys: Vec<Vec<String>>,
    /// Non-unique indexes in declaration order, from `#[db(index)]`.
    pub indexes: Vec<IndexAnnotation>,
}

/// When a trigger fires, from `timing = before` or `timing = after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerTiming {
    Before,
    After,
}

/// Whether a trigger fires once per row or once per statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TriggerLevel {
    Row,
    Statement,
}

/// A statement that fires a trigger, from `events(...)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TriggerEvent {
    Insert,
    Update,
    Delete,
    Truncate,
}

/// A trigger declaration after the declaring model's table fills in its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerRef {
    pub name: String,
    pub table: String,
}

/// A full trigger definition after the declaring model's table fills in its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerDefinition {
    pub name: String,
    pub table: String,
    pub timing: TriggerTiming,
    pub events: Vec<TriggerEvent>,
    pub level: TriggerLevel,
    pub condition: Option<String>,
    pub function: String,
    pub language: String,
    pub body: String,
}

#[derive(Debug)]
struct TriggerInput {
    name: String,
    table: Option<String>,
    definition: Option<TriggerDefinitionInput>,
    span: proc_macro2::Span,
}

#[derive(Debug)]
struct TriggerDefinitionInput {
    timing: TriggerTiming,
    events: Vec<TriggerEvent>,
    level: TriggerLevel,
    condition: Option<String>,
    function: Option<String>,
    language: String,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationCardinality {
    One,
    Many,
}

#[derive(Debug, Clone)]
pub struct RelationAnnotation {
    pub field: Ident,
    pub model: String,
    pub cardinality: RelationCardinality,
    pub table: String,
    pub local_key: Vec<String>,
    pub target_key: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ForeignKeyAnnotation {
    pub table: String,
    pub column: String,
    pub scope: Option<String>,
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKeyAction {
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

#[derive(Debug, Clone)]
pub struct ResolvedForeignKey {
    pub columns: Vec<String>,
    pub table: String,
    pub referenced_columns: Vec<String>,
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexAnnotation {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug)]
struct IndexInput {
    name: Option<String>,
    columns: Vec<String>,
    span: proc_macro2::Span,
}

#[derive(Debug)]
pub enum Column {
    Scalar {
        name: Ident,
        ty: Type,
        nullable: bool,
        pg_type: PgType,
        /// Column default as DDL spells it, e.g. `now()`, `0`, `'{}'::jsonb`.
        pg_default: PgDefault,
        /// Operators the column may be filtered with, in declaration order.
        query_ops: Vec<FilterOperator>,
        /// Whether the column may be ordered by, from `#[db(orderable)]`.
        orderable: bool,
        /// The generated-column expression from `#[db(pg_virtual = "...")]`.
        generated: Option<String>,
        /// Triggers that fill this column, from `#[db(pg_trigger)]`.
        triggers: Vec<TriggerRef>,
    },
    /// A `#[db(flatten)]` field, contributing the columns of its type.
    Flattened { name: Ident, ty: Type },
}

/// Where a column type comes from.
#[derive(Debug)]
pub enum PgType {
    /// Declared on the field with `#[db(pgtype = "...")]`.
    Declared(String),
    /// Taken from the field type's `dbkit::DbType` implementation.
    Inherited,
}

/// Where a column default comes from.
#[derive(Debug)]
pub enum PgDefault {
    /// Declared on the field with `#[db(pgdefault = "...")]`.
    Declared(String),
    /// The field declares no default by omitting `pgdefault`.
    None,
    /// Taken from the field type's `dbkit::DbType` implementation with `#[db(pgdefault = inherit)]`.
    Inherited,
}

/// What the field's `pgdefault` annotation says, before a `PgDefault` is produced.
#[derive(Debug)]
enum FieldDefault {
    Inherit,
    Declared(String),
}

#[derive(Debug, Default)]
struct FieldAnnotations {
    roles: Vec<Role>,
    flatten: bool,
    pg_type: Option<String>,
    pg_default: Option<FieldDefault>,
    query_ops: Vec<FilterOperator>,
    orderable: bool,
    relation: Option<RelationAnnotation>,
    foreign_key: Option<ForeignKeyAnnotation>,
    pg_virtual: Option<String>,
    triggers: Vec<TriggerInput>,
    unique: bool,
}

const ROLES: &[(&str, Role)] = &[
    ("primary_key", Role::PrimaryKey),
    ("version", Role::Version),
    ("created_at", Role::CreatedAt),
    ("updated_at", Role::UpdatedAt),
    ("tenant", Role::Tenant),
];

const FILTER_OPERATORS: &[(&str, FilterOperator)] = &[
    ("eq", FilterOperator::Eq),
    ("neq", FilterOperator::Neq),
    ("gt", FilterOperator::Gt),
    ("gte", FilterOperator::Gte),
    ("lt", FilterOperator::Lt),
    ("lte", FilterOperator::Lte),
    ("like", FilterOperator::Like),
    ("ilike", FilterOperator::ILike),
    ("not_like", FilterOperator::NotLike),
    ("not_ilike", FilterOperator::NotILike),
    ("any", FilterOperator::Any),
    ("not_any", FilterOperator::NotAny),
    ("contains", FilterOperator::Contains),
    ("not_contains", FilterOperator::NotContains),
    ("overlaps", FilterOperator::Overlaps),
    ("not_overlaps", FilterOperator::NotOverlaps),
    ("contains_all", FilterOperator::ContainsAll),
    ("not_contains_all", FilterOperator::NotContainsAll),
    ("is_null", FilterOperator::IsNull),
    ("is_not_null", FilterOperator::IsNotNull),
    ("between", FilterOperator::Between),
    ("not_between", FilterOperator::NotBetween),
];

pub fn parse(input: &DeriveInput) -> syn::Result<Annotations> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "`DbModel` can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(
            data.fields.span(),
            "`DbModel` requires named fields",
        ));
    };

    let mut errors = Vec::new();
    let mut trigger_inputs = Vec::new();
    let mut struct_unique_keys = Vec::new();
    let mut index_inputs = Vec::new();
    let shape = shape(
        &input.attrs,
        input.span(),
        &mut trigger_inputs,
        &mut struct_unique_keys,
        &mut index_inputs,
        &mut errors,
    );
    let fragment = matches!(shape, Some(Shape::Fragment));
    let subquery = matches!(shape, Some(Shape::SubQuery(_)));
    let table = matches!(shape, Some(Shape::Table(_)));
    let model_table = match &shape {
        Some(Shape::Table(table)) => table.as_str(),
        Some(Shape::SubQuery(_)) | Some(Shape::Fragment) | None => "",
    };
    let mut triggers = Vec::new();
    let mut trigger_definitions = Vec::new();
    resolve_triggers(
        trigger_inputs,
        model_table,
        &mut triggers,
        &mut trigger_definitions,
        &mut errors,
    );

    if !fragment && !input.generics.params.is_empty() {
        errors.push(syn::Error::new(
            input.generics.span(),
            "`DbModel` on a table or a subquery cannot have generic parameters; the metadata \
             cache is one static per type",
        ));
    }

    let mut roles = Vec::new();
    let mut columns = Vec::new();
    let mut relations = Vec::new();
    let mut foreign_key_fields = Vec::new();
    let mut field_unique_keys = Vec::new();
    for field in &fields.named {
        let name = field.ident.clone().expect("named field");
        let mut annotations = field_annotations(field, &mut errors);

        if annotations.flatten {
            if fragment {
                errors.push(syn::Error::new(
                    field.span(),
                    "a fragment cannot `#[db(flatten)]` another struct",
                ));
            }
            if !annotations.roles.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with a role",
                ));
            }
            if annotations.pg_type.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `pgtype`",
                ));
            }
            if annotations.pg_default.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `pgdefault`",
                ));
            }
            if !annotations.query_ops.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `query_ops`",
                ));
            }
            if annotations.orderable {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `orderable`",
                ));
            }
            if annotations.relation.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `relation`",
                ));
            }
            if annotations.foreign_key.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `foreign_key`",
                ));
            }
            if annotations.pg_virtual.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `pg_virtual`",
                ));
            }
            if !annotations.triggers.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `pg_trigger`",
                ));
            }
            if annotations.unique {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(flatten)]` cannot be combined with `unique`",
                ));
            }
            if is(&field.ty, "Option") {
                errors.push(syn::Error::new(
                    field.ty.span(),
                    "`#[db(flatten)]` field must not be `Option`",
                ));
            }

            columns.push(Column::Flattened {
                name,
                ty: field.ty.clone(),
            });
            continue;
        }

        if let Some(relation) = annotations.relation.take() {
            if !table {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` is only legal on a table row",
                ));
            }
            if !annotations.roles.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with a role",
                ));
            }
            if annotations.pg_type.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `pgtype`",
                ));
            }
            if annotations.pg_default.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `pgdefault`",
                ));
            }
            if !annotations.query_ops.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `query_ops`",
                ));
            }
            if annotations.orderable {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `orderable`",
                ));
            }
            if annotations.foreign_key.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `foreign_key`",
                ));
            }
            if annotations.pg_virtual.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `pg_virtual`",
                ));
            }
            if !annotations.triggers.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `pg_trigger`",
                ));
            }
            if annotations.unique {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(relation)]` cannot be combined with `unique`",
                ));
            }

            let (inner, many) = match relation_field(&field.ty) {
                Some(relation_field) => relation_field,
                None => {
                    errors.push(syn::Error::new(
                        field.ty.span(),
                        "`#[db(relation)]` field must be `Option<T>` or `Vec<T>`",
                    ));
                    continue;
                }
            };
            let matches_cardinality = matches!(
                (relation.cardinality, many),
                (RelationCardinality::One, false) | (RelationCardinality::Many, true)
            );
            if !matches_cardinality {
                errors.push(syn::Error::new(
                    field.ty.span(),
                    "`#[db(relation(one))]` needs `Option<T>` and `relation(many)` needs `Vec<T>`",
                ));
            }

            relations.push(RelationAnnotation {
                field: name,
                model: type_name(&inner),
                cardinality: relation.cardinality,
                table: relation.table,
                local_key: relation.local_key,
                target_key: relation.target_key,
            });
            continue;
        }

        let field_roles = &annotations.roles;
        let primary_key_and_tenant = field_roles.len() == 2
            && field_roles.contains(&Role::PrimaryKey)
            && field_roles.contains(&Role::Tenant);
        if field_roles.len() > 1 && !primary_key_and_tenant {
            errors.push(syn::Error::new(
                field.span(),
                "a field can carry at most one `#[db(...)]` role, except `primary_key` and \
                 `tenant` together",
            ));
        }
        if fragment && !field_roles.is_empty() {
            errors.push(syn::Error::new(
                field.span(),
                "a fragment cannot carry `#[db(...)]` roles",
            ));
        }
        if subquery && !field_roles.is_empty() {
            errors.push(syn::Error::new(
                field.span(),
                "a subquery cannot carry `#[db(...)]` roles",
            ));
        }
        if field_roles.contains(&Role::Tenant) && annotations.pg_default.is_some() {
            errors.push(syn::Error::new(
                field.span(),
                "`#[db(tenant)]` field must not have a default",
            ));
        }
        if fragment || subquery {
            if annotations.pg_virtual.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(pg_virtual)]` is only legal on a table row",
                ));
            }
            if annotations.foreign_key.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(foreign_key)]` is only legal on a table row",
                ));
            }
            if annotations.unique {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(unique)]` is only legal on a table row",
                ));
            }
        }
        if annotations.pg_virtual.is_some() {
            if !field_roles.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(pg_virtual)]` cannot be combined with a role",
                ));
            }
            if annotations.pg_default.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(pg_virtual)]` cannot be combined with `pgdefault`",
                ));
            }
        }
        if !annotations.triggers.is_empty() {
            if annotations.pg_virtual.is_some() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(pg_trigger)]` cannot be combined with `pg_virtual`",
                ));
            }
            if !field_roles.is_empty() {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(pg_trigger)]` cannot be combined with a role",
                ));
            }
        }
        let field_triggers = resolve_triggers(
            std::mem::take(&mut annotations.triggers),
            model_table,
            &mut triggers,
            &mut trigger_definitions,
            &mut errors,
        );
        if let Some(foreign_key) = annotations.foreign_key.take() {
            foreign_key_fields.push((name.clone(), foreign_key));
        }
        if annotations.unique {
            if field_roles.contains(&Role::PrimaryKey) {
                errors.push(syn::Error::new(
                    field.span(),
                    "`#[db(unique)]` is redundant on a primary-key field",
                ));
            }
            field_unique_keys.push(vec![plain_name(&name)]);
        }

        for &role in field_roles {
            check_field_type(role, field, &mut errors);
            roles.push((role, name.clone(), field.ty.clone()));
        }

        let pg_type = match &annotations.pg_type {
            Some(pg_type) => PgType::Declared(pg_type.clone()),
            None => PgType::Inherited,
        };
        let pg_default = match &annotations.pg_default {
            Some(FieldDefault::Inherit) => PgDefault::Inherited,
            Some(FieldDefault::Declared(default)) => PgDefault::Declared(default.clone()),
            None => PgDefault::None,
        };

        columns.push(Column::Scalar {
            name,
            ty: field.ty.clone(),
            nullable: is(&field.ty, "Option"),
            pg_type,
            pg_default,
            query_ops: annotations.query_ops.clone(),
            orderable: annotations.orderable,
            generated: annotations.pg_virtual.clone(),
            triggers: field_triggers,
        });
    }

    let primary_key: Vec<Ident> = roles
        .iter()
        .filter(|(role, _, _)| *role == Role::PrimaryKey)
        .map(|(_, name, _)| name.clone())
        .collect();
    let version = unique(&roles, Role::Version, &mut errors);
    let created_at = unique(&roles, Role::CreatedAt, &mut errors);
    let updated_at = unique(&roles, Role::UpdatedAt, &mut errors);
    let tenant = unique(&roles, Role::Tenant, &mut errors);

    if matches!(shape, Some(Shape::Table(_)))
        && !roles.iter().any(|(role, _, _)| *role == Role::PrimaryKey)
    {
        errors.push(syn::Error::new(
            input.span(),
            "missing `#[db(primary_key)]`",
        ));
    }

    let mut foreign_keys = Vec::new();
    for (field, foreign_key) in foreign_key_fields {
        let ForeignKeyAnnotation {
            table,
            column,
            scope,
            on_delete,
            on_update,
        } = foreign_key;
        let field_name = plain_name(&field);
        let (columns, referenced_columns) = match scope {
            Some(scope) => {
                if scope == field_name {
                    errors.push(syn::Error::new(
                        field.span(),
                        "`scope` must name another scalar column, not the annotated field",
                    ));
                }
                if !is_scalar_column(&columns, &scope) {
                    errors.push(syn::Error::new(
                        field.span(),
                        format!("`scope` `{scope}` is not a scalar column"),
                    ));
                }
                (vec![scope.clone(), field_name], vec![scope, column])
            }
            None => (vec![field_name], vec![column]),
        };

        foreign_keys.push(ResolvedForeignKey {
            columns,
            table,
            referenced_columns,
            on_delete,
            on_update,
        });
    }

    for relation in &relations {
        if relation.local_key.len() != relation.target_key.len() {
            errors.push(syn::Error::new(
                relation.field.span(),
                "`local_key` and `target_key` must have the same length",
            ));
        }
        for key in &relation.local_key {
            if !is_scalar_column(&columns, key) {
                errors.push(syn::Error::new(
                    relation.field.span(),
                    format!("`local_key` `{key}` is not a scalar column"),
                ));
            }
        }
    }

    if !table && !triggers.is_empty() {
        errors.push(syn::Error::new(
            input.span(),
            "`#[db(pg_trigger)]` is only legal on a table row",
        ));
    }

    let mut unique_keys = struct_unique_keys;
    unique_keys.extend(field_unique_keys);
    if !table && !unique_keys.is_empty() {
        errors.push(syn::Error::new(
            input.span(),
            "`#[db(unique)]` is only legal on a table row",
        ));
    }
    let primary_key_names: Vec<String> = primary_key.iter().map(plain_name).collect();
    let mut seen_unique_keys: Vec<Vec<String>> = Vec::new();
    for key in &unique_keys {
        if key.is_empty() {
            errors.push(syn::Error::new(
                input.span(),
                "`#[db(unique(columns = [...]))]` needs at least one column",
            ));
        }
        let mut seen = Vec::new();
        for column in key {
            if !is_scalar_column(&columns, column) {
                errors.push(syn::Error::new(
                    input.span(),
                    format!("`unique` column `{column}` is not a scalar column"),
                ));
            }
            if seen.contains(column) {
                errors.push(syn::Error::new(
                    input.span(),
                    format!("`unique` key repeats column `{column}`"),
                ));
            }
            seen.push(column.clone());
        }

        let mut sorted = key.clone();
        sorted.sort();
        let mut primary = primary_key_names.clone();
        primary.sort();
        if !sorted.is_empty() && sorted == primary {
            errors.push(syn::Error::new(
                input.span(),
                "a `unique` key cannot repeat the primary key",
            ));
        }
        if seen_unique_keys.contains(&sorted) {
            errors.push(syn::Error::new(input.span(), "duplicate `unique` key"));
        }
        seen_unique_keys.push(sorted);
    }

    let indexes = resolve_indexes(index_inputs, model_table, &columns, &mut errors);
    if !table && !indexes.is_empty() {
        errors.push(syn::Error::new(
            input.span(),
            "`#[db(index)]` is only legal on a table row",
        ));
    }

    combine(errors)?;

    let Some(shape) = shape else {
        unreachable!("missing required annotations are reported as errors");
    };

    Ok(Annotations {
        shape,
        primary_key,
        version,
        created_at,
        updated_at,
        tenant,
        columns,
        relations,
        foreign_keys,
        triggers,
        trigger_definitions,
        unique_keys,
        indexes,
    })
}

/// Metadata declared by `#[derive(DbType)]` on a value type.
#[derive(Debug)]
pub struct TypeAnnotations {
    pub pg_type: String,
    pub pg_default: Option<String>,
}

/// Parses the struct annotations of a `#[derive(DbType)]` type definition.
pub fn parse_type(input: &DeriveInput) -> syn::Result<TypeAnnotations> {
    let mut errors = Vec::new();

    if !matches!(&input.data, Data::Struct(_)) {
        errors.push(syn::Error::new(
            input.span(),
            "`DbType` can only be derived for structs",
        ));
    }
    if let Data::Struct(data) = &input.data {
        for field in &data.fields {
            for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("db")) {
                errors.push(syn::Error::new(
                    attr.span(),
                    "`DbType` takes its annotations on the struct, not on a field",
                ));
            }
        }
    }

    let mut pg_type = None;
    let mut pg_default = None;
    for attr in input.attrs.iter().filter(|attr| attr.path().is_ident("db")) {
        let result = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("pgtype") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() {
                    return Err(meta.error("`pgtype` must not be empty"));
                }
                if pg_type.replace(value.value()).is_some() {
                    return Err(meta.error("duplicate `pgtype`"));
                }
            } else if meta.path.is_ident("pgdefault") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() {
                    return Err(meta.error("`pgdefault` must not be empty"));
                }
                if pg_default.replace(value.value()).is_some() {
                    return Err(meta.error("duplicate `pgdefault`"));
                }
            } else {
                return Err(
                    meta.error("unknown attribute; the struct accepts `pgtype` or `pgdefault`")
                );
            }

            Ok(())
        });

        if let Err(error) = result {
            errors.push(error);
        }
    }

    if pg_type.is_none() {
        errors.push(syn::Error::new(
            input.span(),
            "missing `#[db(pgtype = \"...\")]`",
        ));
    }

    combine(errors)?;

    Ok(TypeAnnotations {
        pg_type: pg_type.expect("a missing `pgtype` is reported as an error"),
        pg_default,
    })
}

fn shape(
    attrs: &[Attribute],
    span: proc_macro2::Span,
    triggers: &mut Vec<TriggerInput>,
    unique_keys: &mut Vec<Vec<String>>,
    indexes: &mut Vec<IndexInput>,
    errors: &mut Vec<syn::Error>,
) -> Option<Shape> {
    let mut shape = None;

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("db")) {
        let result = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() {
                    return Err(meta.error("`table` must not be empty"));
                }
                if shape.is_some() {
                    return Err(meta.error(shape_conflict(&shape, "table")));
                }
                shape = Some(Shape::Table(value.value()));
            } else if meta.path.is_ident("subquery") {
                let value: LitStr = meta.value()?.parse()?;
                let sql = value.value();
                let trimmed = sql.trim();
                if trimmed.is_empty() {
                    return Err(meta.error("`subquery` must not be empty"));
                }
                if trimmed.ends_with(';') {
                    return Err(meta.error(
                        "`subquery` must not end with a semicolon; the query is wrapped in \
                         parentheses",
                    ));
                }
                if shape.is_some() {
                    return Err(meta.error(shape_conflict(&shape, "subquery")));
                }
                shape = Some(Shape::SubQuery(sql));
            } else if meta.path.is_ident("fragment") {
                if shape.is_some() {
                    return Err(meta.error(shape_conflict(&shape, "fragment")));
                }
                shape = Some(Shape::Fragment);
            } else if meta.path.is_ident("pg_trigger") {
                triggers.push(parse_trigger(meta)?);
            } else if meta.path.is_ident("unique") {
                let mut columns = None;
                meta.parse_nested_meta(|option| {
                    if !option.path.is_ident("columns") {
                        return Err(option.error(
                            "unknown `unique` option; the struct form accepts `columns = [...]`",
                        ));
                    }
                    if columns.is_some() {
                        return Err(option.error("duplicate `unique` columns"));
                    }
                    columns = Some(parse_key_list(option.value()?)?);
                    Ok(())
                })?;
                let Some(columns) = columns else {
                    return Err(
                        meta.error("the struct form of `unique` requires `columns = [...]`")
                    );
                };
                unique_keys.push(columns);
            } else if meta.path.is_ident("index") {
                indexes.push(parse_index(meta)?);
            } else {
                return Err(meta.error(
                    "unknown attribute; the struct accepts `table`, `subquery`, `fragment`, \
                     `pg_trigger`, `unique` or `index`",
                ));
            }

            Ok(())
        });

        if let Err(error) = result {
            errors.push(error);
        }
    }

    if shape.is_none() {
        errors.push(syn::Error::new(
            span,
            "missing `#[db(table = \"...\")]`, `#[db(subquery = \"...\")]` or `#[db(fragment)]`",
        ));
    }

    shape
}

/// The error for a repeated or a conflicting shape annotation.
fn shape_conflict(shape: &Option<Shape>, attribute: &str) -> String {
    match (shape, attribute) {
        (Some(Shape::Table(_)), "table") => "duplicate `table`".to_owned(),
        (Some(Shape::SubQuery(_)), "subquery") => "duplicate `subquery`".to_owned(),
        (Some(Shape::Fragment), "fragment") => "duplicate `#[db(fragment)]`".to_owned(),
        _ => "`table`, `subquery` and `fragment` are mutually exclusive".to_owned(),
    }
}

fn field_annotations(field: &Field, errors: &mut Vec<syn::Error>) -> FieldAnnotations {
    let mut annotations = FieldAnnotations::default();

    for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("db")) {
        let mut seen = 0;

        let result = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("pgtype") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() {
                    return Err(meta.error("`pgtype` must not be empty"));
                }
                if annotations.pg_type.replace(value.value()).is_some() {
                    return Err(meta.error("duplicate `pgtype`"));
                }
            } else if meta.path.is_ident("pgdefault") {
                if annotations.pg_default.is_some() {
                    return Err(meta.error("duplicate `pgdefault`"));
                }
                if !meta.input.peek(syn::Token![=]) {
                    return Err(meta.error(
                        "bare `pgdefault` is not supported; omit it for no default or use \
                         `pgdefault = inherit`",
                    ));
                }

                let input = meta.value()?;
                if input.peek(LitStr) {
                    let value: LitStr = input.parse()?;
                    if value.value().is_empty() {
                        return Err(meta.error("`pgdefault` must not be empty"));
                    }
                    annotations.pg_default = Some(FieldDefault::Declared(value.value()));
                } else {
                    let value: Ident = input.parse().map_err(|_| {
                        input.error("expected `inherit` or a string literal after `pgdefault =`")
                    })?;
                    if value == "inherit" {
                        annotations.pg_default = Some(FieldDefault::Inherit);
                    } else {
                        return Err(syn::Error::new(
                            value.span(),
                            "expected `inherit` or a string literal after `pgdefault =`",
                        ));
                    }
                }
            } else if meta.path.is_ident("query_ops") {
                meta.parse_nested_meta(|operator| {
                    let Some((_, filter)) = FILTER_OPERATORS
                        .iter()
                        .find(|(name, _)| operator.path.is_ident(name))
                    else {
                        return Err(operator.error(
                            "unknown query operator; expected `eq`, `neq`, `gt`, `gte`, `lt`, \
                             `lte`, `like`, `ilike`, `not_like`, `not_ilike`, `any`, `not_any`, \
                             `contains`, `not_contains`, `overlaps`, `not_overlaps`, \
                             `contains_all`, `not_contains_all`, `is_null`, `is_not_null`, \
                             `between` or `not_between`",
                        ));
                    };

                    if annotations.query_ops.contains(filter) {
                        return Err(operator.error("duplicate query operator"));
                    }

                    annotations.query_ops.push(*filter);

                    Ok(())
                })?;
            } else if meta.path.is_ident("flatten") {
                if annotations.flatten {
                    return Err(meta.error("duplicate `flatten`"));
                }
                annotations.flatten = true;
            } else if meta.path.is_ident("orderable") {
                if annotations.orderable {
                    return Err(meta.error("duplicate `orderable`"));
                }
                annotations.orderable = true;
            } else if meta.path.is_ident("pg_virtual") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() {
                    return Err(meta.error("`pg_virtual` must not be empty"));
                }
                if annotations.pg_virtual.replace(value.value()).is_some() {
                    return Err(meta.error("duplicate `pg_virtual`"));
                }
            } else if meta.path.is_ident("unique") {
                if annotations.unique {
                    return Err(meta.error("duplicate `unique`"));
                }
                annotations.unique = true;
            } else if meta.path.is_ident("relation") {
                if annotations.relation.is_some() {
                    return Err(meta.error("duplicate `relation`"));
                }
                let mut cardinality = None;
                let mut table = None;
                let mut local_key = None;
                let mut target_key = None;
                meta.parse_nested_meta(|option| {
                    if option.path.is_ident("one") {
                        if cardinality.replace(RelationCardinality::One).is_some() {
                            return Err(option.error("duplicate relation cardinality"));
                        }
                    } else if option.path.is_ident("many") {
                        if cardinality.replace(RelationCardinality::Many).is_some() {
                            return Err(option.error("duplicate relation cardinality"));
                        }
                    } else if option.path.is_ident("table") {
                        let value: LitStr = option.value()?.parse()?;
                        if value.value().is_empty() {
                            return Err(option.error("`table` must not be empty"));
                        }
                        if table.replace(value.value()).is_some() {
                            return Err(option.error("duplicate `table`"));
                        }
                    } else if option.path.is_ident("local_key") {
                        if local_key.is_some() {
                            return Err(option.error("duplicate `local_key`"));
                        }
                        local_key = Some(parse_key_list(option.value()?)?);
                    } else if option.path.is_ident("target_key") {
                        if target_key.is_some() {
                            return Err(option.error("duplicate `target_key`"));
                        }
                        target_key = Some(parse_key_list(option.value()?)?);
                    } else {
                        return Err(option.error(
                            "unknown `relation` option; expected `one`, `many`, `table`, \
                             `local_key` or `target_key`",
                        ));
                    }

                    Ok(())
                })?;

                let Some(cardinality) = cardinality else {
                    return Err(meta.error("`relation` requires `one` or `many`"));
                };
                let Some(table) = table else {
                    return Err(meta.error("`relation` requires `table = \"...\"`"));
                };
                let Some(local_key) = local_key else {
                    return Err(meta.error("`relation` requires `local_key`"));
                };
                let Some(target_key) = target_key else {
                    return Err(meta.error("`relation` requires `target_key`"));
                };
                if local_key.len() != target_key.len() {
                    return Err(
                        meta.error("`local_key` and `target_key` must have the same length")
                    );
                }

                let model = relation_field(&field.ty)
                    .map(|(inner, _)| type_name(&inner))
                    .unwrap_or_default();
                annotations.relation = Some(RelationAnnotation {
                    field: field.ident.clone().expect("named field"),
                    model,
                    cardinality,
                    table,
                    local_key,
                    target_key,
                });
            } else if meta.path.is_ident("foreign_key") {
                if annotations.foreign_key.is_some() {
                    return Err(meta.error("duplicate `foreign_key`"));
                }
                let mut table = None;
                let mut column = None;
                let mut scope = None;
                let mut on_delete = None;
                let mut on_update = None;
                meta.parse_nested_meta(|option| {
                    if option.path.is_ident("table") {
                        let value: LitStr = option.value()?.parse()?;
                        if value.value().is_empty() {
                            return Err(option.error("`table` must not be empty"));
                        }
                        if table.replace(value.value()).is_some() {
                            return Err(option.error("duplicate `table`"));
                        }
                    } else if option.path.is_ident("column") {
                        let value: LitStr = option.value()?.parse()?;
                        if value.value().is_empty() {
                            return Err(option.error("`column` must not be empty"));
                        }
                        if column.replace(value.value()).is_some() {
                            return Err(option.error("duplicate `column`"));
                        }
                    } else if option.path.is_ident("scope") {
                        let value: LitStr = option.value()?.parse()?;
                        if value.value().is_empty() {
                            return Err(option.error("`scope` must not be empty"));
                        }
                        if scope.replace(value.value()).is_some() {
                            return Err(option.error("duplicate `scope`"));
                        }
                    } else if option.path.is_ident("on_delete") {
                        let value: Ident = option.value()?.parse()?;
                        let action = parse_foreign_key_action(&value)?;
                        if on_delete.replace(action).is_some() {
                            return Err(option.error("duplicate `on_delete`"));
                        }
                    } else if option.path.is_ident("on_update") {
                        let value: Ident = option.value()?.parse()?;
                        let action = parse_foreign_key_action(&value)?;
                        if on_update.replace(action).is_some() {
                            return Err(option.error("duplicate `on_update`"));
                        }
                    } else {
                        return Err(option.error(
                            "unknown `foreign_key` option; expected `table`, `column`, `scope`, \
                             `on_delete` or `on_update`",
                        ));
                    }

                    Ok(())
                })?;

                let Some(table) = table else {
                    return Err(meta.error("`foreign_key` requires `table = \"...\"`"));
                };
                let Some(column) = column else {
                    return Err(meta.error("`foreign_key` requires `column = \"...\"`"));
                };
                annotations.foreign_key = Some(ForeignKeyAnnotation {
                    table,
                    column,
                    scope,
                    on_delete: on_delete.unwrap_or(ForeignKeyAction::NoAction),
                    on_update: on_update.unwrap_or(ForeignKeyAction::NoAction),
                });
            } else if meta.path.is_ident("pg_trigger") {
                annotations.triggers.push(parse_trigger(meta)?);
            } else {
                let Some((_, role)) = ROLES.iter().find(|(name, _)| meta.path.is_ident(name))
                else {
                    return Err(meta.error(
                        "unknown attribute; expected `primary_key`, `version`, `created_at`, \
                         `updated_at`, `tenant`, `flatten`, `pgtype`, `pgdefault`, `query_ops`, \
                         `orderable` or `pg_trigger`",
                    ));
                };

                annotations.roles.push(*role);
            }

            seen += 1;

            Ok(())
        });

        match result {
            Ok(()) if seen == 0 => {
                errors.push(syn::Error::new(attr.span(), "empty `#[db]` attribute"))
            }
            Ok(()) => {}
            Err(error) => errors.push(error),
        }
    }

    annotations
}

fn parse_trigger(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<TriggerInput> {
    let span = meta.path.span();

    if meta.input.peek(syn::Token![=]) {
        let value: LitStr = meta.value()?.parse()?;
        let name = value.value();
        if name.is_empty() {
            return Err(meta.error("`pg_trigger` must not be empty"));
        }

        return Ok(TriggerInput {
            name,
            table: None,
            definition: None,
            span,
        });
    }

    let mut name = None;
    let mut table = None;
    let mut timing = None;
    let mut events = None;
    let mut level = None;
    let mut condition = None;
    let mut function = None;
    let mut language = None;
    let mut body = None;

    meta.parse_nested_meta(|option| {
        if option.path.is_ident("name") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.is_empty() {
                return Err(option.error("`name` must not be empty"));
            }
            if name.replace(value).is_some() {
                return Err(option.error("duplicate `name`"));
            }
        } else if option.path.is_ident("on") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.is_empty() {
                return Err(option.error("`on` must not be empty"));
            }
            if table.replace(value).is_some() {
                return Err(option.error("duplicate `on`"));
            }
        } else if option.path.is_ident("timing") {
            let value: Ident = option.value()?.parse()?;
            let value = match value.to_string().as_str() {
                "before" => TriggerTiming::Before,
                "after" => TriggerTiming::After,
                _ => {
                    return Err(syn::Error::new(
                        value.span(),
                        "expected `before` or `after` after `timing =`",
                    ));
                }
            };
            if timing.replace(value).is_some() {
                return Err(option.error("duplicate `timing`"));
            }
        } else if option.path.is_ident("events") {
            if events.is_some() {
                return Err(option.error("duplicate `events`"));
            }
            let mut list = Vec::new();
            option.parse_nested_meta(|event| {
                let parsed = match event
                    .path
                    .get_ident()
                    .map(|ident| ident.to_string())
                    .as_deref()
                {
                    Some("insert") => TriggerEvent::Insert,
                    Some("update") => TriggerEvent::Update,
                    Some("delete") => TriggerEvent::Delete,
                    Some("truncate") => TriggerEvent::Truncate,
                    _ => {
                        return Err(event.error(
                            "unknown trigger event; expected `insert`, `update`, `delete` or \
                             `truncate`",
                        ));
                    }
                };
                if list.contains(&parsed) {
                    return Err(event.error("duplicate trigger event"));
                }
                list.push(parsed);
                Ok(())
            })?;
            events = Some(list);
        } else if option.path.is_ident("level") {
            let value: Ident = option.value()?.parse()?;
            let value = match value.to_string().as_str() {
                "row" => TriggerLevel::Row,
                "statement" => TriggerLevel::Statement,
                _ => {
                    return Err(syn::Error::new(
                        value.span(),
                        "expected `row` or `statement` after `level =`",
                    ));
                }
            };
            if level.replace(value).is_some() {
                return Err(option.error("duplicate `level`"));
            }
        } else if option.path.is_ident("condition") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.is_empty() {
                return Err(option.error("`condition` must not be empty"));
            }
            if condition.replace(value).is_some() {
                return Err(option.error("duplicate `condition`"));
            }
        } else if option.path.is_ident("function") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.is_empty() {
                return Err(option.error("`function` must not be empty"));
            }
            if function.replace(value).is_some() {
                return Err(option.error("duplicate `function`"));
            }
        } else if option.path.is_ident("language") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.is_empty() {
                return Err(option.error("`language` must not be empty"));
            }
            if language.replace(value).is_some() {
                return Err(option.error("duplicate `language`"));
            }
        } else if option.path.is_ident("body") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.trim().is_empty() {
                return Err(option.error("`body` must not be empty"));
            }
            if body.replace(value).is_some() {
                return Err(option.error("duplicate `body`"));
            }
        } else {
            return Err(option.error(
                "unknown `pg_trigger` option; expected `name`, `on`, `timing`, `events`, `level`, \
                 `condition`, `function`, `language` or `body`",
            ));
        }

        Ok(())
    })?;

    let Some(name) = name else {
        return Err(meta.error("`pg_trigger` requires `name = \"...\"`"));
    };

    let definition = match body {
        Some(body) => {
            let Some(timing) = timing else {
                return Err(
                    meta.error("`pg_trigger` requires `timing = before` or `timing = after`")
                );
            };
            let Some(mut events) = events else {
                return Err(meta.error("`pg_trigger` requires `events(...)`"));
            };
            events.sort();
            let level = level.unwrap_or(TriggerLevel::Row);
            if level == TriggerLevel::Statement && condition.is_some() {
                return Err(meta.error("`condition` is only legal on a row-level trigger"));
            }
            if events.contains(&TriggerEvent::Truncate) {
                if level == TriggerLevel::Row {
                    return Err(meta.error("`truncate` requires `level = statement`"));
                }
                if condition.is_some() {
                    return Err(meta.error("`truncate` cannot have a `condition`"));
                }
            }

            Some(TriggerDefinitionInput {
                timing,
                events,
                level,
                condition,
                function,
                language: language.unwrap_or_else(|| "plpgsql".to_owned()),
                body,
            })
        }
        None => {
            if timing.is_some()
                || events.is_some()
                || level.is_some()
                || condition.is_some()
                || function.is_some()
                || language.is_some()
            {
                return Err(meta.error(
                    "a `pg_trigger` without `body` is a reference; it accepts only `name` and `on`",
                ));
            }

            None
        }
    };

    Ok(TriggerInput {
        name,
        table,
        definition,
        span,
    })
}

fn resolve_triggers(
    inputs: Vec<TriggerInput>,
    model_table: &str,
    triggers: &mut Vec<TriggerRef>,
    definitions: &mut Vec<TriggerDefinition>,
    errors: &mut Vec<syn::Error>,
) -> Vec<TriggerRef> {
    let mut field_refs = Vec::new();

    for input in inputs {
        let TriggerInput {
            name,
            table,
            definition,
            span,
        } = input;
        let table = table.unwrap_or_else(|| model_table.to_owned());
        let reference = TriggerRef {
            name: name.clone(),
            table: table.clone(),
        };

        if !triggers
            .iter()
            .any(|existing| existing.name == reference.name && existing.table == reference.table)
        {
            triggers.push(reference.clone());
        }
        if !field_refs.iter().any(|existing: &TriggerRef| {
            existing.name == reference.name && existing.table == reference.table
        }) {
            field_refs.push(reference);
        }

        let Some(definition) = definition else {
            continue;
        };
        let resolved = TriggerDefinition {
            function: definition.function.unwrap_or_else(|| name.clone()),
            name,
            table,
            timing: definition.timing,
            events: definition.events,
            level: definition.level,
            condition: definition.condition,
            language: definition.language,
            body: definition.body,
        };

        match definitions
            .iter()
            .find(|existing| existing.name == resolved.name && existing.table == resolved.table)
        {
            Some(existing) if existing != &resolved => errors.push(syn::Error::new(
                span,
                format!(
                    "conflicting `pg_trigger` definitions for `{}` on table `{}`",
                    resolved.name, resolved.table,
                ),
            )),
            Some(_) => {}
            None => definitions.push(resolved),
        }
    }

    field_refs
}

fn parse_foreign_key_action(value: &Ident) -> syn::Result<ForeignKeyAction> {
    match value.to_string().as_str() {
        "no_action" => Ok(ForeignKeyAction::NoAction),
        "restrict" => Ok(ForeignKeyAction::Restrict),
        "cascade" => Ok(ForeignKeyAction::Cascade),
        "set_null" => Ok(ForeignKeyAction::SetNull),
        "set_default" => Ok(ForeignKeyAction::SetDefault),
        _ => Err(syn::Error::new(
            value.span(),
            "expected `no_action`, `restrict`, `cascade`, `set_null` or `set_default`",
        )),
    }
}

fn parse_index(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<IndexInput> {
    let span = meta.path.span();
    let mut name = None;
    let mut columns = None;

    meta.parse_nested_meta(|option| {
        if option.path.is_ident("columns") {
            if columns.is_some() {
                return Err(option.error("duplicate `columns`"));
            }
            columns = Some(parse_key_list(option.value()?)?);
        } else if option.path.is_ident("name") {
            let value: LitStr = option.value()?.parse()?;
            let value = value.value();
            if value.is_empty() {
                return Err(option.error("`name` must not be empty"));
            }
            if name.replace(value).is_some() {
                return Err(option.error("duplicate `name`"));
            }
        } else {
            return Err(option.error("unknown `index` option; expected `columns` or `name`"));
        }

        Ok(())
    })?;

    let Some(columns) = columns else {
        return Err(meta.error("`index` requires `columns = [...]`"));
    };

    Ok(IndexInput {
        name,
        columns,
        span,
    })
}

fn resolve_indexes(
    inputs: Vec<IndexInput>,
    model_table: &str,
    columns: &[Column],
    errors: &mut Vec<syn::Error>,
) -> Vec<IndexAnnotation> {
    let mut indexes: Vec<IndexAnnotation> = Vec::new();

    for input in inputs {
        let IndexInput {
            name,
            columns: index_columns,
            span,
        } = input;
        let mut seen = Vec::new();
        for column in &index_columns {
            if !is_scalar_column(columns, column) {
                errors.push(syn::Error::new(
                    span,
                    format!("`index` column `{column}` is not a scalar column"),
                ));
            }
            if seen.contains(column) {
                errors.push(syn::Error::new(
                    span,
                    format!("`index` repeats column `{column}`"),
                ));
            }
            seen.push(column.clone());
        }

        let name = name.unwrap_or_else(|| {
            let mut name = model_table.to_owned();
            for column in &index_columns {
                name.push('_');
                name.push_str(column);
            }
            name.push_str("_idx");
            name
        });
        if indexes
            .iter()
            .any(|existing| existing.name == name || existing.columns == index_columns)
        {
            errors.push(syn::Error::new(span, "duplicate `index`"));
        }

        indexes.push(IndexAnnotation {
            name,
            columns: index_columns,
        });
    }

    indexes
}

fn check_field_type(role: Role, field: &Field, errors: &mut Vec<syn::Error>) {
    let name = match role {
        Role::PrimaryKey => "primary_key",
        Role::Version => "version",
        Role::CreatedAt => "created_at",
        Role::UpdatedAt => "updated_at",
        Role::Tenant => "tenant",
    };

    if is(&field.ty, "Option") {
        errors.push(syn::Error::new(
            field.ty.span(),
            format!("`#[db({name})]` field must not be `Option`"),
        ));
    }

    match role {
        Role::Version if !is(&field.ty, "i64") => errors.push(syn::Error::new(
            field.ty.span(),
            "`#[db(version)]` field must be `i64`",
        )),
        Role::CreatedAt | Role::UpdatedAt if !is(&field.ty, "DateTime") => {
            errors.push(syn::Error::new(
                field.ty.span(),
                format!("`#[db({name})]` field must be a `DateTime`"),
            ));
        }
        _ => {}
    }
}

/// Returns the single field with `role`, reporting every extra occurrence.
fn unique(
    roles: &[(Role, Ident, Type)],
    role: Role,
    errors: &mut Vec<syn::Error>,
) -> Option<Ident> {
    let mut found = roles.iter().filter(|(found, _, _)| *found == role);
    let first = found.next()?.1.clone();

    for (_, duplicate, _) in found {
        errors.push(syn::Error::new(
            duplicate.span(),
            "duplicate `#[db(...)]` role",
        ));
    }

    Some(first)
}

fn is(ty: &Type, name: &str) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };

    segment.ident == name
}

/// The inner type of a relation field, and whether it is a `Vec` rather than an `Option`.
fn relation_field(ty: &Type) -> Option<(Type, bool)> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    let many = match segment.ident.to_string().as_str() {
        "Option" => false,
        "Vec" => true,
        _ => return None,
    };
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let inner = arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })?;

    Some((inner, many))
}

/// The last path segment of a type, used as a relation's model name.
fn type_name(ty: &Type) -> String {
    let Type::Path(path) = ty else {
        return ty.to_token_stream().to_string();
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

/// A field or column name without a raw-identifier prefix.
fn plain_name(name: &Ident) -> String {
    name.to_string().trim_start_matches("r#").to_owned()
}

/// Whether `name` is a scalar column of the model.
fn is_scalar_column(columns: &[Column], name: &str) -> bool {
    columns.iter().any(|column| match column {
        Column::Scalar { name: field, .. } => plain_name(field) == name,
        Column::Flattened { .. } => false,
    })
}

/// One identifier or a bracketed, comma-separated identifier list, as used by relation keys and unique keys.
fn parse_key_list(input: syn::parse::ParseStream) -> syn::Result<Vec<String>> {
    let mut keys = Vec::new();

    if input.peek(syn::token::Bracket) {
        let content;
        syn::bracketed!(content in input);
        while !content.is_empty() {
            let key: Ident = content.parse()?;
            keys.push(plain_name(&key));
            if content.peek(syn::Token![,]) {
                content.parse::<syn::Token![,]>()?;
            } else {
                break;
            }
        }
    } else {
        let key: Ident = input.parse()?;
        keys.push(plain_name(&key));
    }

    if keys.is_empty() {
        return Err(input.error("expected at least one column name"));
    }

    Ok(keys)
}

/// The type inside an `Option`, or the type itself.
pub fn underlying(ty: &Type) -> &Type {
    let Type::Path(path) = ty else {
        return ty;
    };
    let Some(segment) = path.path.segments.last() else {
        return ty;
    };
    if segment.ident != "Option" {
        return ty;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return ty;
    };

    arguments
        .args
        .iter()
        .find_map(|argument| match argument {
            syn::GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .unwrap_or(ty)
}

fn combine(errors: Vec<syn::Error>) -> syn::Result<()> {
    let mut errors = errors.into_iter();
    let Some(mut combined) = errors.next() else {
        return Ok(());
    };

    for error in errors {
        combined.combine(error);
    }

    Err(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_model(source: &str) -> syn::Result<Annotations> {
        parse(&syn::parse_str::<DeriveInput>(source).unwrap())
    }

    fn error(source: &str) -> String {
        parse_model(source).unwrap_err().to_string()
    }

    fn names(idents: &[Ident]) -> Vec<String> {
        idents.iter().map(ToString::to_string).collect()
    }

    struct Scalar<'a> {
        name: &'a Ident,
        nullable: bool,
        pg_type: &'a PgType,
        pg_default: &'a PgDefault,
        query_ops: &'a [FilterOperator],
        orderable: bool,
    }

    impl PgType {
        /// The declared type, or `None` when inherited from the field type.
        fn declared(&self) -> Option<&str> {
            match self {
                PgType::Declared(pg_type) => Some(pg_type),
                PgType::Inherited => None,
            }
        }
    }

    impl PgDefault {
        /// The field's own default: `Some(None)` for none declared, `None` when inherited from the type.
        fn declared(&self) -> Option<Option<&str>> {
            match self {
                PgDefault::Declared(pg_default) => Some(Some(pg_default)),
                PgDefault::None => Some(None),
                PgDefault::Inherited => None,
            }
        }
    }

    fn scalar(column: &Column) -> Scalar<'_> {
        let Column::Scalar {
            name,
            nullable,
            pg_type,
            pg_default,
            query_ops,
            orderable,
            ..
        } = column
        else {
            panic!("expected a scalar column");
        };

        Scalar {
            name,
            nullable: *nullable,
            pg_type,
            pg_default,
            query_ops,
            orderable: *orderable,
        }
    }

    const VALID: &str = r#"
        #[db(table = "email")]
        struct Email {
            #[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]
            id: EmailId,
            #[db(pgtype = "text")]
            subject: String,
            #[db(created_at, pgtype = "timestamptz", pgdefault = "now()")]
            created_at: DateTime<Utc>,
            #[db(updated_at, pgtype = "timestamptz", pgdefault = "now()")]
            updated_at: DateTime<Utc>,
            #[db(version, pgtype = "int8", pgdefault = "0")]
            version: i64,
        }
    "#;

    #[test]
    fn reads_annotations() {
        let model = parse_model(VALID).unwrap();

        assert_eq!(model.shape, Shape::Table("email".to_owned()));
        assert_eq!(names(&model.primary_key), ["id"]);
        assert_eq!(model.version.unwrap().to_string(), "version");
        assert_eq!(model.created_at.unwrap().to_string(), "created_at");
        assert_eq!(model.updated_at.unwrap().to_string(), "updated_at");

        let definitions: Vec<(&str, Option<&str>)> = model
            .columns
            .iter()
            .map(|column| {
                let column = scalar(column);
                (
                    column.pg_type.declared().unwrap(),
                    column.pg_default.declared().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            definitions,
            [
                ("uuid", Some("uuidv7()")),
                ("text", None),
                ("timestamptz", Some("now()")),
                ("timestamptz", Some("now()")),
                ("int8", Some("0")),
            ]
        );
    }

    #[test]
    fn reads_composite_primary_key_in_declaration_order() {
        let model = parse_model(
            r#"
            #[db(table = "membership")]
            struct Membership {
                #[db(primary_key, pgtype = "uuid")]
                user_id: UserId,
                #[db(pgtype = "text")]
                team: String,
                #[db(primary_key, pgtype = "uuid")]
                team_id: TeamId,
                #[db(version, pgtype = "int8")]
                version: i64,
            }
        "#,
        )
        .unwrap();

        assert_eq!(names(&model.primary_key), ["user_id", "team_id"]);
    }

    #[test]
    fn reads_columns_in_declaration_order() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                name: String,
                #[db(pgtype = "text")]
                note: Option<String>,
            }
        "#,
        )
        .unwrap();

        let columns: Vec<(String, bool, &str)> = model
            .columns
            .iter()
            .map(|column| {
                let column = scalar(column);
                (
                    column.name.to_string(),
                    column.nullable,
                    column.pg_type.declared().unwrap(),
                )
            })
            .collect();

        assert_eq!(
            columns,
            [
                ("id".to_owned(), false, "uuid"),
                ("name".to_owned(), false, "text"),
                ("note".to_owned(), true, "text"),
            ]
        );
    }

    #[test]
    fn reads_query_ops_in_declaration_order() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                #[db(query_ops(eq, neq, like, any))]
                name: String,
                #[db(pgtype = "text")]
                #[db(query_ops(between))]
                note: Option<String>,
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            scalar(&model.columns[1]).query_ops,
            [
                FilterOperator::Eq,
                FilterOperator::Neq,
                FilterOperator::Like,
                FilterOperator::Any,
            ]
        );
        assert_eq!(
            scalar(&model.columns[2]).query_ops,
            [FilterOperator::Between]
        );
        assert!(scalar(&model.columns[0]).query_ops.is_empty());
    }

    #[test]
    fn reads_every_query_operator() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                #[db(query_ops(
                    eq, neq, gt, gte, lt, lte, like, ilike, not_like, not_ilike, any, not_any,
                    contains, not_contains, overlaps, not_overlaps, contains_all,
                    not_contains_all, is_null, is_not_null, between, not_between
                ))]
                name: String,
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            scalar(&model.columns[1]).query_ops,
            [
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
            ]
        );
    }

    #[test]
    fn reads_orderable() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                #[db(orderable)]
                name: String,
                #[db(pgtype = "text")]
                note: Option<String>,
            }
        "#,
        )
        .unwrap();

        assert!(scalar(&model.columns[1]).orderable);
        assert!(!scalar(&model.columns[0]).orderable);
        assert!(!scalar(&model.columns[2]).orderable);
    }

    #[test]
    fn rejects_duplicate_orderable() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text", orderable, orderable)]
                name: String,
            }
        "#,
        );

        assert!(message.contains("duplicate `orderable`"), "{message}");
    }

    #[test]
    fn rejects_duplicate_query_operators() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                #[db(query_ops(eq, eq))]
                name: String,
            }
        "#,
        );

        assert!(message.contains("duplicate query operator"), "{message}");
    }

    #[test]
    fn rejects_empty_query_ops() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                #[db(query_ops())]
                name: String,
            }
        "#,
        );

        assert!(message.contains("expected nested attribute"), "{message}");
    }

    #[test]
    fn rejects_unknown_query_operator() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                #[db(query_ops(eq, approximately))]
                name: String,
            }
        "#,
        );

        assert!(message.contains("unknown query operator"), "{message}");
    }

    #[test]
    fn version_and_audit_columns_are_optional() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
            }
        "#,
        )
        .unwrap();

        assert!(model.version.is_none());
        assert!(model.created_at.is_none());
        assert!(model.updated_at.is_none());
    }

    #[test]
    fn requires_table_or_fragment() {
        let message = error(
            r#"
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains(
                "missing `#[db(table = \"...\")]`, `#[db(subquery = \"...\")]` or \
                 `#[db(fragment)]`"
            ),
            "{message}"
        );
    }

    fn column_names(columns: &[Column]) -> Vec<String> {
        columns
            .iter()
            .map(|column| match column {
                Column::Scalar { name, .. } | Column::Flattened { name, .. } => name.to_string(),
            })
            .collect()
    }

    #[test]
    fn reads_a_fragment() {
        let fragment = parse_model(
            r#"
            #[db(fragment)]
            struct Address {
                #[db(pgtype = "text")]
                street: String,
                #[db(pgtype = "text")]
                city: Option<String>,
            }
        "#,
        )
        .unwrap();

        assert_eq!(fragment.shape, Shape::Fragment);
        assert!(fragment.primary_key.is_empty());
        assert!(fragment.version.is_none());
        assert_eq!(column_names(&fragment.columns), ["street", "city"]);
        assert!(scalar(&fragment.columns[1]).nullable);
    }

    #[test]
    fn reads_a_subquery() {
        let model = parse_model(
            r#"
            #[db(subquery = "SELECT id, name FROM widget")]
            struct WidgetSummary {
                #[db(pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "text")]
                name: String,
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            model.shape,
            Shape::SubQuery("SELECT id, name FROM widget".to_owned())
        );
        assert!(model.primary_key.is_empty());
        assert!(model.version.is_none());
        assert_eq!(column_names(&model.columns), ["id", "name"]);
    }

    #[test]
    fn rejects_an_empty_subquery() {
        let message = error(
            r#"
            #[db(subquery = "  ")]
            struct WidgetSummary {
                #[db(pgtype = "uuid")]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("`subquery` must not be empty"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_subquery_with_a_trailing_semicolon() {
        let message = error(
            r#"
            #[db(subquery = "SELECT id FROM widget;")]
            struct WidgetSummary {
                #[db(pgtype = "uuid")]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("`subquery` must not end with a semicolon"),
            "{message}"
        );
    }

    #[test]
    fn rejects_subquery_with_another_shape() {
        let message = error(
            r#"
            #[db(table = "widget")]
            #[db(subquery = "SELECT id FROM widget")]
            struct WidgetSummary {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("`table`, `subquery` and `fragment` are mutually exclusive"),
            "{message}"
        );
    }

    #[test]
    fn rejects_roles_in_a_subquery() {
        let message = error(
            r#"
            #[db(subquery = "SELECT id FROM widget")]
            struct WidgetSummary {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("a subquery cannot carry `#[db(...)]` roles"),
            "{message}"
        );
    }

    #[test]
    fn reads_flattened_fields_in_declaration_order() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(flatten)]
                address: Address,
                #[db(pgtype = "text")]
                note: Option<String>,
            }
        "#,
        )
        .unwrap();

        assert_eq!(column_names(&model.columns), ["id", "address", "note"]);
        assert!(matches!(model.columns[1], Column::Flattened { .. }));
    }

    #[test]
    fn rejects_table_and_fragment_together() {
        let message = error(
            r#"
            #[db(table = "widget")]
            #[db(fragment)]
            struct Widget {
                #[db(pgtype = "text")]
                note: String,
            }
        "#,
        );

        assert!(
            message.contains("`table`, `subquery` and `fragment` are mutually exclusive"),
            "{message}"
        );
    }

    #[test]
    fn rejects_roles_in_a_fragment() {
        let message = error(
            r#"
            #[db(fragment)]
            struct Audit {
                #[db(created_at, pgtype = "timestamptz")]
                created_at: DateTime<Utc>,
            }
        "#,
        );

        assert!(
            message.contains("a fragment cannot carry `#[db(...)]` roles"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_flattened_field_with_a_column_annotation() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(flatten, pgtype = "text")]
                address: Address,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(flatten)]` cannot be combined with `pgtype`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_flattened_field_with_orderable() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(flatten, orderable)]
                address: Address,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(flatten)]` cannot be combined with `orderable`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_an_optional_flattened_field() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(flatten)]
                address: Option<Address>,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(flatten)]` field must not be `Option`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_flatten_in_a_fragment() {
        let message = error(
            r#"
            #[db(fragment)]
            struct Address {
                #[db(flatten)]
                location: Location,
            }
        "#,
        );

        assert!(
            message.contains("a fragment cannot `#[db(flatten)]` another struct"),
            "{message}"
        );
    }

    #[test]
    fn rejects_generic_tables() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget<T> {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "jsonb")]
                payload: T,
            }
        "#,
        );

        assert!(
            message.contains("cannot have generic parameters"),
            "{message}"
        );
    }

    #[test]
    fn requires_primary_key() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(version, pgtype = "int8")]
                version: i64,
            }
        "#,
        );

        assert!(
            message.contains("missing `#[db(primary_key)]`"),
            "{message}"
        );
    }

    #[test]
    fn resolves_pgtype_and_pgdefault_independently() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key)]
                id: WidgetId,
                #[db(pgdefault = inherit)]
                inherited_default: WidgetId,
                #[db(pgtype = "uuid")]
                own_type: WidgetId,
                #[db(pgtype = "uuid", pgdefault = "gen_random_uuid()")]
                own_type_and_default: WidgetId,
                #[db(pgtype = "uuid", pgdefault = inherit)]
                own_type_and_inherited_default: WidgetId,
            }
        "#,
        )
        .unwrap();

        let columns: Vec<(Option<&str>, Option<Option<&str>>)> = model
            .columns
            .iter()
            .map(|column| {
                let column = scalar(column);
                (column.pg_type.declared(), column.pg_default.declared())
            })
            .collect();

        assert_eq!(
            columns,
            [
                (None, Some(None)),
                (None, None),
                (Some("uuid"), Some(None)),
                (Some("uuid"), Some(Some("gen_random_uuid()"))),
                (Some("uuid"), None),
            ]
        );
    }

    #[test]
    fn rejects_empty_pgtype() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "")]
                id: WidgetId,
            }
        "#,
        );

        assert!(message.contains("`pgtype` must not be empty"), "{message}");
    }

    #[test]
    fn rejects_duplicate_pgtype() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid", pgtype = "text")]
                id: WidgetId,
            }
        "#,
        );

        assert!(message.contains("duplicate `pgtype`"), "{message}");
    }

    #[test]
    fn rejects_empty_pgdefault() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid", pgdefault = "")]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("`pgdefault` must not be empty"),
            "{message}"
        );
    }

    #[test]
    fn rejects_duplicate_pgdefault() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]
                #[db(pgdefault = "gen_random_uuid()")]
                id: WidgetId,
            }
        "#,
        );

        assert!(message.contains("duplicate `pgdefault`"), "{message}");
    }

    #[test]
    fn rejects_bare_pgdefault() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                #[db(pgdefault)]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("bare `pgdefault` is not supported"),
            "{message}"
        );
    }

    #[test]
    fn rejects_none_pgdefault() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid", pgdefault = none)]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            message.contains("expected `inherit` or a string literal"),
            "{message}"
        );
    }

    #[test]
    fn rejects_duplicate_roles() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(version, pgtype = "int8")]
                version: i64,
                #[db(version, pgtype = "int8")]
                other_version: i64,
            }
        "#,
        );

        assert!(message.contains("duplicate `#[db(...)]` role"), "{message}");
    }

    #[test]
    fn reports_primary_key_repeated_on_one_field_as_multiple_roles() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                #[db(primary_key)]
                id: WidgetId,
                #[db(version, pgtype = "int8")]
                version: i64,
            }
        "#,
        );

        assert!(message.contains("at most one"), "{message}");
    }

    #[test]
    fn rejects_unknown_attributes() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(pgtype = "timestamptz", created)]
                created_at: DateTime<Utc>,
                #[db(version, pgtype = "int8")]
                version: i64,
            }
        "#,
        );

        assert!(message.contains("unknown attribute"), "{message}");
    }

    #[test]
    fn rejects_optional_role_fields() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(created_at, pgtype = "timestamptz")]
                created_at: Option<DateTime<Utc>>,
                #[db(version, pgtype = "int8")]
                version: i64,
            }
        "#,
        );

        assert!(message.contains("must not be `Option`"), "{message}");
    }

    #[test]
    fn rejects_optional_primary_key_field() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Option<WidgetId>,
                #[db(version, pgtype = "int8")]
                version: i64,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(primary_key)]` field must not be `Option`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_wrong_role_types() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(version, pgtype = "int4")]
                version: i32,
            }
        "#,
        );

        assert!(message.contains("must be `i64`"), "{message}");
    }

    #[test]
    fn rejects_multiple_roles_on_one_field() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, version, pgtype = "int8")]
                id: i64,
            }
        "#,
        );

        assert!(message.contains("at most one"), "{message}");
    }

    #[test]
    fn rejects_non_structs() {
        let message = error(r#"#[db(table = "widget")] enum Widget { A }"#);

        assert!(message.contains("only be derived for structs"), "{message}");
    }

    fn parse_type_error(source: &str) -> String {
        parse_type(&syn::parse_str::<DeriveInput>(source).unwrap())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn reads_type_annotations() {
        let annotations = parse_type(
            &syn::parse_str::<DeriveInput>(
                r#"
                #[db(pgtype = "uuid")]
                #[db(pgdefault = "uuidv7()")]
                struct WidgetId(uuid::Uuid);
            "#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(annotations.pg_type, "uuid");
        assert_eq!(annotations.pg_default.as_deref(), Some("uuidv7()"));
    }

    #[test]
    fn type_requires_pgtype() {
        let message = parse_type_error(
            r#"
            struct WidgetId(uuid::Uuid);
        "#,
        );

        assert!(
            message.contains("missing `#[db(pgtype = \"...\")]`"),
            "{message}"
        );
    }

    #[test]
    fn type_rejects_field_annotations() {
        let message = parse_type_error(
            r#"
            #[db(pgtype = "uuid")]
            struct WidgetId(#[db(pgtype = "text")] uuid::Uuid);
        "#,
        );

        assert!(
            message.contains("takes its annotations on the struct"),
            "{message}"
        );
    }

    #[test]
    fn type_rejects_model_annotations() {
        let message = parse_type_error(
            r#"
            #[db(table = "widget")]
            struct WidgetId(uuid::Uuid);
        "#,
        );

        assert!(
            message.contains("the struct accepts `pgtype` or `pgdefault`"),
            "{message}"
        );
    }

    #[test]
    fn reads_the_tenant_role() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(tenant, pgtype = "uuid")]
                tenant_id: TenantId,
                #[db(pgtype = "text")]
                name: String,
            }
        "#,
        )
        .unwrap();

        assert_eq!(model.tenant.unwrap().to_string(), "tenant_id");
    }

    #[test]
    fn allows_tenant_and_primary_key_on_one_column() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, tenant, pgtype = "uuid")]
                tenant_id: TenantId,
                #[db(pgtype = "text")]
                name: String,
            }
        "#,
        )
        .unwrap();

        assert_eq!(names(&model.primary_key), ["tenant_id"]);
        assert_eq!(model.tenant.unwrap().to_string(), "tenant_id");
    }

    #[test]
    fn rejects_a_defaulted_tenant_column() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(tenant, pgtype = "uuid", pgdefault = "uuidv7()")]
                tenant_id: TenantId,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(tenant)]` field must not have a default"),
            "{message}"
        );
    }

    #[test]
    fn rejects_an_optional_tenant_column() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: WidgetId,
                #[db(tenant, pgtype = "uuid")]
                tenant_id: Option<TenantId>,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(tenant)]` field must not be `Option`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_relation_with_the_wrong_field_type() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(relation(one, table = "other", local_key = id, target_key = id))]
                other: Vec<Other>,
            }
        "#,
        );

        assert!(message.contains("needs `Option<T>`"), "{message}");
    }

    #[test]
    fn rejects_a_relation_local_key_that_is_not_a_column() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(relation(one, table = "other", local_key = missing, target_key = id))]
                other: Option<Other>,
            }
        "#,
        );

        assert!(
            message.contains("`local_key` `missing` is not a scalar column"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_foreign_key_scope_that_is_not_a_column() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pgtype = "uuid", foreign_key(table = "other", column = "id", scope = "missing"))]
                other_id: Uuid,
            }
        "#,
        );

        assert!(
            message.contains("`scope` `missing` is not a scalar column"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_trigger_on_a_fragment() {
        let message = error(
            r#"
            #[db(fragment)]
            #[db(pg_trigger = "fragment_trigger")]
            struct Address {
                #[db(pgtype = "text")]
                street: String,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(pg_trigger)]` is only legal on a table row"),
            "{message}"
        );
    }

    #[test]
    fn rejects_pg_virtual_on_a_role_field() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid", pg_virtual = "1")]
                id: Uuid,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(pg_virtual)]` cannot be combined with a role"),
            "{message}"
        );
    }

    #[test]
    fn rejects_unique_on_a_primary_key() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid", unique)]
                id: Uuid,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(unique)]` is redundant on a primary-key field"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_struct_unique_with_an_unknown_column() {
        let message = error(
            r#"
            #[db(table = "widget")]
            #[db(unique(columns = [id, missing]))]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
            }
        "#,
        );

        assert!(
            message.contains("`unique` column `missing` is not a scalar column"),
            "{message}"
        );
    }

    #[test]
    fn reads_a_field_trigger_definition() {
        let model = parse_model(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_trigger(
                    name = "invoice_totals",
                    timing = before,
                    events(update, insert),
                    body = "begin return new; end;"
                ))]
                net_amount: i64,
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            model.triggers,
            [TriggerRef {
                name: "invoice_totals".to_owned(),
                table: "invoice".to_owned(),
            }]
        );
        assert_eq!(model.trigger_definitions.len(), 1);
        let trigger = &model.trigger_definitions[0];
        assert_eq!(trigger.name, "invoice_totals");
        assert_eq!(trigger.table, "invoice");
        assert_eq!(trigger.timing, TriggerTiming::Before);
        assert_eq!(trigger.events, [TriggerEvent::Insert, TriggerEvent::Update]);
        assert_eq!(trigger.level, TriggerLevel::Row);
        assert_eq!(trigger.function, "invoice_totals");
        assert_eq!(trigger.language, "plpgsql");
        assert_eq!(trigger.body, "begin return new; end;");

        let Column::Scalar { triggers, .. } = &model.columns[1] else {
            panic!("expected a scalar column");
        };
        assert_eq!(triggers, &model.triggers);
    }

    #[test]
    fn reads_a_trigger_installed_on_another_table() {
        let model = parse_model(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_trigger(name = "invoice_row_totals", on = "invoice_row"))]
                net_amount: i64,
                #[db(pg_trigger = "invoice_totals")]
                tax_amount: i64,
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            model.triggers,
            [
                TriggerRef {
                    name: "invoice_row_totals".to_owned(),
                    table: "invoice_row".to_owned(),
                },
                TriggerRef {
                    name: "invoice_totals".to_owned(),
                    table: "invoice".to_owned(),
                },
            ]
        );
        assert!(model.trigger_definitions.is_empty());
    }

    #[test]
    fn rejects_conflicting_trigger_definitions() {
        let message = error(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_trigger(
                    name = "invoice_totals",
                    timing = before,
                    events(insert),
                    body = "begin return new; end;"
                ))]
                net_amount: i64,
                #[db(pg_trigger(
                    name = "invoice_totals",
                    timing = before,
                    events(update),
                    body = "begin return new; end;"
                ))]
                tax_amount: i64,
            }
        "#,
        );

        assert!(
            message.contains("conflicting `pg_trigger` definitions"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_trigger_reference_with_definition_options() {
        let message = error(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_trigger(name = "invoice_totals", timing = before))]
                net_amount: i64,
            }
        "#,
        );

        assert!(
            message.contains("without `body` is a reference"),
            "{message}"
        );
    }

    #[test]
    fn rejects_empty_trigger_events() {
        let message = error(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_trigger(
                    name = "invoice_totals",
                    timing = before,
                    events(),
                    body = "begin return new; end;"
                ))]
                net_amount: i64,
            }
        "#,
        );

        assert!(message.contains("expected nested attribute"), "{message}");
    }

    #[test]
    fn rejects_a_row_level_truncate_trigger() {
        let message = error(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_trigger(
                    name = "invoice_truncate",
                    timing = before,
                    events(truncate),
                    body = "begin return null; end;"
                ))]
                net_amount: i64,
            }
        "#,
        );

        assert!(
            message.contains("`truncate` requires `level = statement`"),
            "{message}"
        );
    }

    #[test]
    fn reads_indexes_with_default_names() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            #[db(index(columns = [name]))]
            #[db(index(columns = [tenant_id, name], name = "widget_tenant_name_idx"))]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pgtype = "uuid")]
                tenant_id: Uuid,
                #[db(pgtype = "text")]
                name: String,
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            model.indexes,
            [
                IndexAnnotation {
                    name: "widget_name_idx".to_owned(),
                    columns: vec!["name".to_owned()],
                },
                IndexAnnotation {
                    name: "widget_tenant_name_idx".to_owned(),
                    columns: vec!["tenant_id".to_owned(), "name".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn rejects_an_index_on_a_missing_column() {
        let message = error(
            r#"
            #[db(table = "widget")]
            #[db(index(columns = [missing]))]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
            }
        "#,
        );

        assert!(
            message.contains("`index` column `missing` is not a scalar column"),
            "{message}"
        );
    }

    #[test]
    fn reads_foreign_key_actions() {
        let model = parse_model(
            r#"
            #[db(table = "child")]
            struct Child {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(foreign_key(
                    table = "parent",
                    column = "id",
                    on_delete = cascade,
                    on_update = set_null
                ))]
                parent_id: Uuid,
            }
        "#,
        )
        .unwrap();

        assert_eq!(model.foreign_keys.len(), 1);
        assert_eq!(model.foreign_keys[0].on_delete, ForeignKeyAction::Cascade);
        assert_eq!(model.foreign_keys[0].on_update, ForeignKeyAction::SetNull);
    }

    #[test]
    fn rejects_an_unknown_foreign_key_action() {
        let message = error(
            r#"
            #[db(table = "child")]
            struct Child {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(foreign_key(table = "parent", column = "id", on_delete = explode))]
                parent_id: Uuid,
            }
        "#,
        );

        assert!(
            message.contains("expected `no_action`, `restrict`, `cascade`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_trigger_on_a_virtual_column() {
        let message = error(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(pg_virtual = "1")]
                #[db(pg_trigger = "invoice_totals")]
                net_amount: i64,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(pg_trigger)]` cannot be combined with `pg_virtual`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_a_trigger_on_a_role_field() {
        let message = error(
            r#"
            #[db(table = "invoice")]
            struct Invoice {
                #[db(primary_key, pgtype = "uuid")]
                id: Uuid,
                #[db(version, pgtype = "int8")]
                #[db(pg_trigger = "invoice_totals")]
                version: i64,
            }
        "#,
        );

        assert!(
            message.contains("`#[db(pg_trigger)]` cannot be combined with a role"),
            "{message}"
        );
    }
}
