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

#[derive(Debug)]
// Read by code generation; the derive only validates the annotations for now.
#[allow(dead_code, reason = "code generation lands later")]
pub struct Annotations {
    pub table: String,
    /// Primary key columns in declaration order; never empty.
    pub primary_key: Vec<Ident>,
    pub version: Ident,
    pub created_at: Option<Ident>,
    pub updated_at: Option<Ident>,
}

const ROLES: &[(&str, Role)] = &[
    ("primary_key", Role::PrimaryKey),
    ("version", Role::Version),
    ("created_at", Role::CreatedAt),
    ("updated_at", Role::UpdatedAt),
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
    let table = table(&input.attrs, input.span(), &mut errors);

    let mut roles = Vec::new();
    for field in &fields.named {
        let field_roles = roles_of(field, &mut errors);
        if field_roles.len() > 1 {
            errors.push(syn::Error::new(
                field.span(),
                "a field can carry at most one `#[db(...)]` role",
            ));
        }

        for role in field_roles {
            check_field_type(role, field, &mut errors);
            roles.push((
                role,
                field.ident.clone().expect("named field"),
                field.ty.clone(),
            ));
        }
    }

    let primary_key: Vec<Ident> = roles
        .iter()
        .filter(|(role, _, _)| *role == Role::PrimaryKey)
        .map(|(_, name, _)| name.clone())
        .collect();
    let version = unique(&roles, Role::Version, &mut errors);
    let created_at = unique(&roles, Role::CreatedAt, &mut errors);
    let updated_at = unique(&roles, Role::UpdatedAt, &mut errors);

    for (role, name) in [
        (Role::PrimaryKey, "primary_key"),
        (Role::Version, "version"),
    ] {
        if !roles.iter().any(|(found, _, _)| *found == role) {
            errors.push(syn::Error::new(
                input.span(),
                format!("missing `#[db({name})]`"),
            ));
        }
    }

    combine(errors)?;

    let (Some(table), Some(version)) = (table, version) else {
        unreachable!("missing required annotations are reported as errors");
    };

    Ok(Annotations {
        table,
        primary_key,
        version,
        created_at,
        updated_at,
    })
}

fn table(
    attrs: &[Attribute],
    span: proc_macro2::Span,
    errors: &mut Vec<syn::Error>,
) -> Option<String> {
    let mut table = None;

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("db")) {
        let result = attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("table") {
                return Err(meta.error("unknown attribute; the struct only accepts `table`"));
            }

            let value: LitStr = meta.value()?.parse()?;
            if value.value().is_empty() {
                return Err(meta.error("`table` must not be empty"));
            }
            if table.replace(value.value()).is_some() {
                return Err(meta.error("duplicate `table`"));
            }

            Ok(())
        });

        if let Err(error) = result {
            errors.push(error);
        }
    }

    if table.is_none() {
        errors.push(syn::Error::new(span, "missing `#[db(table = \"...\")]`"));
    }

    table
}

fn roles_of(field: &Field, errors: &mut Vec<syn::Error>) -> Vec<Role> {
    let mut roles = Vec::new();

    for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("db")) {
        let mut seen = 0;

        let result = attr.parse_nested_meta(|meta| {
            let Some((_, role)) = ROLES.iter().find(|(name, _)| meta.path.is_ident(name)) else {
                return Err(meta.error(
                    "unknown attribute; expected `primary_key`, `version`, `created_at` or \
                     `updated_at`",
                ));
            };

            seen += 1;
            roles.push(*role);

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

    roles
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

    const VALID: &str = r#"
        #[db(table = "email")]
        struct Email {
            #[db(primary_key)]
            id: EmailId,
            subject: String,
            #[db(created_at)]
            created_at: DateTime<Utc>,
            #[db(updated_at)]
            updated_at: DateTime<Utc>,
            #[db(version)]
            version: i64,
        }
    "#;

    #[test]
    fn reads_annotations() {
        let model = parse_model(VALID).unwrap();

        assert_eq!(model.table, "email");
        assert_eq!(names(&model.primary_key), ["id"]);
        assert_eq!(model.version.to_string(), "version");
        assert_eq!(model.created_at.unwrap().to_string(), "created_at");
        assert_eq!(model.updated_at.unwrap().to_string(), "updated_at");
    }

    #[test]
    fn reads_composite_primary_key_in_declaration_order() {
        let model = parse_model(
            r#"
            #[db(table = "membership")]
            struct Membership {
                #[db(primary_key)]
                user_id: UserId,
                team: String,
                #[db(primary_key)]
                team_id: TeamId,
                #[db(version)]
                version: i64,
            }
        "#,
        )
        .unwrap();

        assert_eq!(names(&model.primary_key), ["user_id", "team_id"]);
    }

    #[test]
    fn audit_columns_are_optional() {
        let model = parse_model(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key)]
                id: WidgetId,
                #[db(version)]
                version: i64,
            }
        "#,
        )
        .unwrap();

        assert!(model.created_at.is_none());
        assert!(model.updated_at.is_none());
    }

    #[test]
    fn requires_table() {
        let message = error(
            r#"
            struct Widget {
                #[db(primary_key)]
                id: WidgetId,
                #[db(version)]
                version: i64,
            }
        "#,
        );

        assert!(message.contains("missing `#[db(table"), "{message}");
    }

    #[test]
    fn requires_primary_key_and_version() {
        let missing_key = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(version)]
                version: i64,
            }
        "#,
        );

        assert!(
            missing_key.contains("missing `#[db(primary_key)]`"),
            "{missing_key}"
        );

        let missing_version = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key)]
                id: WidgetId,
            }
        "#,
        );

        assert!(
            missing_version.contains("missing `#[db(version)]`"),
            "{missing_version}"
        );
    }

    #[test]
    fn rejects_duplicate_roles() {
        let message = error(
            r#"
            #[db(table = "widget")]
            struct Widget {
                #[db(primary_key)]
                id: WidgetId,
                #[db(version)]
                version: i64,
                #[db(version)]
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
                #[db(primary_key)]
                #[db(primary_key)]
                id: WidgetId,
                #[db(version)]
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
                #[db(primary_key)]
                id: WidgetId,
                #[db(created)]
                created_at: DateTime<Utc>,
                #[db(version)]
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
                #[db(primary_key)]
                id: WidgetId,
                #[db(created_at)]
                created_at: Option<DateTime<Utc>>,
                #[db(version)]
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
                #[db(primary_key)]
                id: Option<WidgetId>,
                #[db(version)]
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
                #[db(primary_key)]
                id: WidgetId,
                #[db(version)]
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
                #[db(primary_key, version)]
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
}
