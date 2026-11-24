//! Filter values: one boxed value per operand, and the one list that names the
//! concrete Rust type of a value arriving as JSON.

use crate::FilterOperator;
use crate::query::Error;
use chrono::DateTime;
use chrono::NaiveDate;
use chrono::Utc;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::Arguments;
use sqlx::Encode;
use sqlx::Type;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgTypeInfo;
use sqlx::postgres::Postgres;
use std::any::Any;
use std::fmt::Debug;
use uuid::Uuid;

/// One bound value: enough to bind it, to compare it in a test and to
/// serialize it back to JSON.
///
/// This is an object-safe shim over sqlx's [`Type`] and [`Encode`], which are
/// not dyn-compatible on their own: `Type::type_info()` is an associated
/// function, so `Box<dyn Type<Postgres>>` does not compile. The blanket impl
/// resolves both traits per concrete `T`, and the trait object only exposes
/// `&self` methods.
///
/// Newtypes need no implementation of their own: a transparent
/// `#[derive(sqlx::Type)]` newtype already reports its inner type, so it is a
/// filter value the moment it is a column value.
pub trait Bind: Debug + Send + Sync + 'static {
    /// Appends the value to the arguments of the statement being built.
    fn bind(&self, arguments: &mut PgArguments) -> Result<(), sqlx::Error>;

    /// The value as JSON, for a readable assertion and for serializing the
    /// query back to the payload it came from.
    fn as_json(&self) -> serde_json::Value;

    /// Type-exact comparison: `100i64` does not equal `100i32`.
    fn eq_dyn(&self, other: &dyn Bind) -> bool;

    #[doc(hidden)]
    fn as_any(&self) -> &dyn Any;
}

impl<T> Bind for T
where
    T: Debug
        + PartialEq
        + Serialize
        + Send
        + Sync
        + 'static
        + Type<Postgres>
        + for<'q> Encode<'q, Postgres>,
{
    fn bind(&self, arguments: &mut PgArguments) -> Result<(), sqlx::Error> {
        arguments.add(self).map_err(sqlx::Error::Encode)
    }

    fn as_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("a filter value serializes")
    }

    fn eq_dyn(&self, other: &dyn Bind) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Whether the column holds an array.
pub(crate) fn is_array(pg_type: &str) -> bool {
    pg_type.trim().ends_with("[]")
}

/// The element type of an array column type, or the type itself.
pub(crate) fn element_pg_type(pg_type: &'static str) -> &'static str {
    match pg_type.trim().strip_suffix("[]") {
        Some(element) => element.trim(),
        None => pg_type.trim(),
    }
}

/// The type name without its array suffix and its length modifier, so
/// `varchar(20)[]` becomes `varchar`.
fn base_name(pg_type: &'static str) -> &'static str {
    let pg_type = element_pg_type(pg_type);

    match pg_type.split_once('(') {
        Some((base, _)) => base.trim(),
        None => pg_type,
    }
}

/// The JSON kind of a value, for an error that says what arrived.
pub(crate) fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// The JSON shape a type parses from, so a wrong shape is reported as the
/// column's type rather than as serde's expectation. This is about JSON, not
/// about SQL types: `bool` reads a boolean, `i64` a number, `uuid` a string.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Bool,
    Number,
    Text,
    /// Whatever arrived, for a `json`/`jsonb` column.
    Any,
}

impl Shape {
    fn accepts(self, value: &Value, array: bool) -> bool {
        if array {
            return value.is_array();
        }

        match self {
            Shape::Bool => value.is_boolean(),
            Shape::Number => value.is_number(),
            Shape::Text => value.is_string(),
            Shape::Any => true,
        }
    }
}

/// The Rust type of a column type, asked of sqlx rather than of a table kept
/// here.
///
/// `Type::compatible` decides which of the types below a name denotes, so the
/// aliases come from sqlx: `String` accepts `text`, `varchar`, `bpchar`,
/// `name` and `citext`, and `i64` accepts only `int8`. Nothing in `dbkit`
/// spells those names out.
///
/// One list, because sqlx can describe a type but cannot build one from JSON:
/// `Type::type_info()` is an associated function, so the concrete type has to
/// be named somewhere, and the value is then parsed by serde. The list is
/// reached only by the JSON face; a Rust-built filter names its type through
/// the handle, so `c.total.eq(500)` never consults it.
///
/// `Type::compatible` panics when asked about an array type it cannot resolve,
/// so an array column is resolved by its element type and boxed as a `Vec`.
fn rust_type(
    pg_type: &'static str,
    array: bool,
    value: &Value,
    column: &'static str,
) -> Option<Result<Box<dyn Bind>, Error>> {
    macro_rules! bindable {
        ($($shape:ident => $ty:ty),* $(,)?) => {
            $(
                let matches = <$ty as Type<Postgres>>::compatible(
                    &PgTypeInfo::with_name(base_name(pg_type)),
                );
                if matches {
                    if !Shape::$shape.accepts(value, array) {
                        return Some(Err(Error::WrongType {
                            column,
                            pg_type: pg_type.to_owned(),
                            actual: describe(value).to_owned(),
                        }));
                    }

                    return Some(if array {
                        from_json::<Vec<$ty>>(value, column, pg_type)
                    } else {
                        from_json::<$ty>(value, column, pg_type)
                    });
                }
            )*
        };
    }

    // `Value` last: it is the fallback for a JSON column and must not shadow a
    // more specific type.
    bindable! {
        Bool => bool,
        Number => i16,
        Number => i32,
        Number => i64,
        Number => f32,
        Number => f64,
        Text => String,
        Text => Uuid,
        Text => NaiveDate,
        Text => DateTime<Utc>,
        Any => Value,
    }

    None
}

/// Parses one JSON value into `T`, blaming the column for a failure.
fn from_json<T: DeserializeOwned + Bind>(
    value: &Value,
    column: &'static str,
    pg_type: &'static str,
) -> Result<Box<dyn Bind>, Error> {
    match T::deserialize(value) {
        Ok(parsed) => Ok(Box::new(parsed)),
        // The shape was checked already, so this is a value that does not
        // parse: serde names what is wrong with it.
        Err(error) => Err(Error::InvalidValue {
            column,
            pg_type: pg_type.to_owned(),
            // The value has no position of its own; the document offset serde
            // appends would point at the outer object.
            reason: without_position(&error),
        }),
    }
}

fn without_position(error: &serde_json::Error) -> String {
    let message = error.to_string();

    match message.find(" at line ") {
        Some(position) => message[..position].to_owned(),
        None => message,
    }
}

fn unsupported(column: &'static str, pg_type: &str) -> Error {
    Error::UnsupportedType {
        column,
        pg_type: pg_type.to_owned(),
    }
}

/// One value of the column's type, boxed, ready to bind as a single parameter.
///
/// An array column boxes the whole array: `{"tags": ["rust"]}` is one
/// parameter of type `text[]`.
pub(crate) fn scalar(
    pg_type: &'static str,
    column: &'static str,
    value: &Value,
) -> Result<Box<dyn Bind>, Error> {
    rust_type(pg_type, is_array(pg_type), value, column)
        .unwrap_or_else(|| Err(unsupported(column, pg_type)))
}

/// A list of the column's element type, boxed as one array parameter, with the
/// element count so an empty list can be rejected.
pub(crate) fn list(
    operator: FilterOperator,
    element_pg_type: &'static str,
    column: &'static str,
    value: &Value,
) -> Result<(Box<dyn Bind>, usize), Error> {
    let Some(items) = value.as_array() else {
        return Err(Error::WrongShape {
            operator,
            expected: "a list",
            actual: describe(value).to_owned(),
        });
    };

    let values = rust_type(element_pg_type, true, value, column)
        .unwrap_or_else(|| Err(unsupported(column, element_pg_type)))?;

    Ok((values, items.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn binds(pg_type: &'static str, value: serde_json::Value) -> Box<dyn Bind> {
        scalar(pg_type, "column", &value).expect("the value parses")
    }

    #[test]
    fn every_alias_sqlx_accepts_parses_as_the_same_type() {
        // The alias list lives in sqlx's `Type::compatible`, not here.
        for pg_type in ["text", "varchar", "varchar(20)", "bpchar", "name"] {
            assert_eq!(
                binds(pg_type, json!("x")).as_json(),
                json!("x"),
                "{pg_type}"
            );
        }
    }

    #[test]
    fn the_type_names_decide_distinctly() {
        assert_eq!(binds("int2", json!(1)).as_json(), json!(1));
        assert_eq!(binds("int4", json!(2)).as_json(), json!(2));
        assert_eq!(binds("int8", json!(3)).as_json(), json!(3));
        assert_eq!(binds("float4", json!(1.5)).as_json(), json!(1.5));
        assert_eq!(binds("float8", json!(2.5)).as_json(), json!(2.5));
        assert_eq!(binds("bool", json!(true)).as_json(), json!(true));
        assert_eq!(
            binds("timestamptz", json!("2024-01-01T00:00:00Z")).as_json(),
            json!("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            binds("date", json!("2024-01-01")).as_json(),
            json!("2024-01-01")
        );
        assert_eq!(
            binds("uuid", json!("00000000-0000-0000-0000-000000000000")).as_json(),
            json!("00000000-0000-0000-0000-000000000000")
        );
    }

    #[test]
    fn an_array_of_the_element_type_is_one_value() {
        let value = binds("text[]", json!(["a", "b"]));

        assert_eq!(value.as_json(), json!(["a", "b"]));
    }

    #[test]
    fn a_type_sqlx_does_not_match_is_unsupported() {
        let error = scalar("numeric(10,2)", "total", &json!(1)).err().unwrap();

        assert_eq!(
            error.to_string(),
            "column `total` is numeric(10,2), which has no JSON filter value"
        );
    }

    #[test]
    fn a_wrong_json_kind_is_reported_as_the_column_type() {
        let error = scalar("uuid", "id", &json!(42)).err().unwrap();

        assert_eq!(
            error.to_string(),
            "column `id` is uuid, the value is a number"
        );
    }

    #[test]
    fn a_malformed_value_has_no_position() {
        let error = scalar("uuid", "id", &json!("not-a-uuid")).err().unwrap();
        let message = error.to_string();

        assert!(!message.contains("line "), "{message}");
        assert!(message.starts_with("column `id` is uuid, "), "{message}");
    }

    #[test]
    fn the_element_type_of_an_array_keeps_its_lifetime() {
        let pg_type: &'static str = "varchar(20)[]";

        assert_eq!(element_pg_type(pg_type), "varchar(20)");
        assert_eq!(base_name(pg_type), "varchar");
    }
}
