//! Parsing and validation of the `#[db(...)]` annotations.

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
}

/// A `dbkit::FilterOperator` variant, known to the macro by its annotation
/// name.
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

/// What the struct describes: a table row, or a group of columns embedded in
/// another model with `#[db(flatten)]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Table(String),
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
    /// Every field in declaration order.
    pub columns: Vec<Column>,
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
    /// The field declares no default; a bare `#[db(pgdefault)]` guarantees
    /// this even when the type has one.
    None,
    /// Taken from the field type's `dbkit::DbType` implementation.
    Inherited,
}

#[derive(Debug, Default)]
struct FieldAnnotations {
    roles: Vec<Role>,
    flatten: bool,
    pg_type: Option<String>,
    pg_default: Option<String>,
    /// A bare `#[db(pgdefault)]`, meaning "no default".
    pg_default_none: bool,
    query_ops: Vec<FilterOperator>,
}

const ROLES: &[(&str, Role)] = &[
    ("primary_key", Role::PrimaryKey),
    ("version", Role::Version),
    ("created_at", Role::CreatedAt),
    ("updated_at", Role::UpdatedAt),
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
    let shape = shape(&input.attrs, input.span(), &mut errors);
    let fragment = matches!(shape, Some(Shape::Fragment));

    if !fragment && !input.generics.params.is_empty() {
        errors.push(syn::Error::new(
            input.generics.span(),
            "`DbModel` on a table cannot have generic parameters; the metadata cache is one \
             static per type",
        ));
    }

    let mut roles = Vec::new();
    let mut columns = Vec::new();
    for field in &fields.named {
        let name = field.ident.clone().expect("named field");
        let annotations = field_annotations(field, &mut errors);

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
            if annotations.pg_default_none {
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

        let field_roles = &annotations.roles;
        if field_roles.len() > 1 {
            errors.push(syn::Error::new(
                field.span(),
                "a field can carry at most one `#[db(...)]` role",
            ));
        }
        if fragment && !field_roles.is_empty() {
            errors.push(syn::Error::new(
                field.span(),
                "a fragment cannot carry `#[db(...)]` roles",
            ));
        }

        for &role in field_roles {
            check_field_type(role, field, &mut errors);
            roles.push((role, name.clone(), field.ty.clone()));
        }

        let pg_type = match &annotations.pg_type {
            Some(pg_type) => PgType::Declared(pg_type.clone()),
            None => PgType::Inherited,
        };
        // Only a field that inherits its type inherits the default: a field
        // spelling out `pgtype` keeps the default it declares itself.
        let pg_default = match &annotations.pg_default {
            Some(default) => PgDefault::Declared(default.clone()),
            None if annotations.pg_default_none || matches!(&pg_type, PgType::Declared(_)) => {
                PgDefault::None
            }
            None => PgDefault::Inherited,
        };

        columns.push(Column::Scalar {
            name,
            ty: field.ty.clone(),
            nullable: is(&field.ty, "Option"),
            pg_type,
            pg_default,
            query_ops: annotations.query_ops.clone(),
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

    if !fragment && !roles.iter().any(|(role, _, _)| *role == Role::PrimaryKey) {
        errors.push(syn::Error::new(
            input.span(),
            "missing `#[db(primary_key)]`",
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
        columns,
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
                match &shape {
                    None => shape = Some(Shape::Table(value.value())),
                    Some(Shape::Table(_)) => return Err(meta.error("duplicate `table`")),
                    Some(Shape::Fragment) => {
                        return Err(meta.error("`table` and `fragment` are mutually exclusive"));
                    }
                }
            } else if meta.path.is_ident("fragment") {
                match &shape {
                    None => shape = Some(Shape::Fragment),
                    Some(Shape::Table(_)) => {
                        return Err(meta.error("`table` and `fragment` are mutually exclusive"));
                    }
                    Some(Shape::Fragment) => {
                        return Err(meta.error("duplicate `#[db(fragment)]`"));
                    }
                }
            } else {
                return Err(
                    meta.error("unknown attribute; the struct accepts `table` or `fragment`")
                );
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
            "missing `#[db(table = \"...\")]` or `#[db(fragment)]`",
        ));
    }

    shape
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
                let duplicate = annotations.pg_default.is_some() || annotations.pg_default_none;
                if !meta.input.peek(syn::Token![=]) {
                    if duplicate {
                        return Err(meta.error("duplicate `pgdefault`"));
                    }
                    annotations.pg_default_none = true;
                } else {
                    let value: LitStr = meta.value()?.parse()?;
                    if value.value().is_empty() {
                        return Err(meta.error("`pgdefault` must not be empty"));
                    }
                    if duplicate {
                        return Err(meta.error("duplicate `pgdefault`"));
                    }
                    annotations.pg_default = Some(value.value());
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
            } else {
                let Some((_, role)) = ROLES.iter().find(|(name, _)| meta.path.is_ident(name))
                else {
                    return Err(meta.error(
                        "unknown attribute; expected `primary_key`, `version`, `created_at`, \
                         `updated_at`, `flatten`, `pgtype`, `pgdefault` or `query_ops`",
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

fn check_field_type(role: Role, field: &Field, errors: &mut Vec<syn::Error>) {
    let name = match role {
        Role::PrimaryKey => "primary_key",
        Role::Version => "version",
        Role::CreatedAt => "created_at",
        Role::UpdatedAt => "updated_at",
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
        /// The field's own default, `Some(None)` when it declares none, or
        /// `None` when the default is inherited from the field type.
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
            message.contains("missing `#[db(table = \"...\")]` or `#[db(fragment)]`"),
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
            message.contains("`table` and `fragment` are mutually exclusive"),
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
    fn inherits_pgtype_and_pgdefault_from_the_field_type() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key)]
                id: WidgetId,
                #[db(pgdefault)]
                cleared_id: WidgetId,
                #[db(pgtype = "uuid")]
                own_type: WidgetId,
                #[db(pgtype = "uuid", pgdefault = "gen_random_uuid()")]
                own_type_and_default: WidgetId,
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
                (None, None),
                (None, Some(None)),
                (Some("uuid"), Some(None)),
                (Some("uuid"), Some(Some("gen_random_uuid()"))),
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
    fn rejects_bare_and_declared_pgdefault() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key, pgtype = "uuid")]
                #[db(pgdefault)]
                #[db(pgdefault = "uuidv7()")]
                id: WidgetId,
            }
        "#,
        );

        assert!(message.contains("duplicate `pgdefault`"), "{message}");
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
}
