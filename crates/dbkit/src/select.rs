//! One field of a projected row, distinguishing "not selected" from "selected
//! `NULL`".

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use sqlx::Column as _;
use sqlx::Decode;
use sqlx::Row;
use sqlx::Type;
use sqlx::postgres::PgRow;
use sqlx::postgres::Postgres;

/// One column of a projected row.
///
/// A projection returns only the columns the request asked for, so a decoded
/// row has to answer "the caller did not ask for this column" separately from
/// "the column is `NULL`". `Patch<T>` draws the same distinction on the way in;
/// this is the way out.
///
/// A nullable column keeps its `Option`: `Selected<Option<T>>`'s `Set(None)` is
/// a selected `NULL`, and `Absent` is a column the projection left out,
/// whatever its nullability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Selected<T> {
    #[default]
    Absent,
    Set(T),
}

impl<T> Selected<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Selected::Absent)
    }

    pub fn is_set(&self) -> bool {
        matches!(self, Selected::Set(_))
    }

    pub fn as_set(&self) -> Option<&T> {
        match self {
            Selected::Set(value) => Some(value),
            Selected::Absent => None,
        }
    }

    pub fn into_set(self) -> Option<T> {
        match self {
            Selected::Set(value) => Some(value),
            Selected::Absent => None,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Selected<U> {
        match self {
            Selected::Set(value) => Selected::Set(f(value)),
            Selected::Absent => Selected::Absent,
        }
    }
}

/// An absent field is omitted from a response, like every other empty value.
impl<T: Serialize> Serialize for Selected<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Unreachable behind `skip_serializing_if`, and the only honest
            // value for a caller that serializes the field anyway.
            Selected::Absent => serializer.serialize_none(),
            Selected::Set(value) => value.serialize(serializer),
        }
    }
}

/// A present value is `Set`; absence comes from the key being missing, which
/// needs `#[serde(default)]` on the field.
///
/// A nullable column keeps its `Option` inside, so `null` deserializes to
/// `Set(None)`: a `NULL` that was selected. A non-nullable column's `null` is a
/// type error, because the column cannot hold one.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Selected<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Selected::Set)
    }
}

/// A projected row, as a whole.
///
/// A row is empty when every column in it is absent, which is what lets a
/// nested `#[db(flatten)]` select skip itself in a response: a fragment with
/// no selected column writes nothing rather than `{}`.
pub trait SelectRow {
    fn is_empty(&self) -> bool;
}

/// One column of a row, by name.
///
/// The row reports its own projection through `columns()`, so a field whose
/// column is not in the result set decodes to [`Selected::Absent`] instead of
/// failing. That is what lets one select struct serve every projection of a
/// table, and it is why a query needs no second parameter to say which columns
/// were asked for.
pub fn selected<'r, T>(row: &'r PgRow, column: &str) -> Result<Selected<T>, sqlx::Error>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    if !row.columns().iter().any(|present| present.name() == column) {
        return Ok(Selected::Absent);
    }

    row.try_get::<T, _>(column).map(Selected::Set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_is_the_default() {
        assert_eq!(Selected::<String>::default(), Selected::Absent);
        assert!(Selected::<String>::Absent.is_absent());
        assert!(Selected::Set("x".to_owned()).is_set());
    }

    #[test]
    fn map_keeps_absence() {
        assert_eq!(
            Selected::<i64>::Absent.map(|value| value + 1),
            Selected::Absent
        );
        assert_eq!(
            Selected::Set(1i64).map(|value| value + 1),
            Selected::Set(2i64)
        );
    }

    #[test]
    fn absent_and_set_serialize_differently() {
        #[derive(Serialize)]
        struct Row {
            #[serde(skip_serializing_if = "Selected::is_absent")]
            name: Selected<String>,
            #[serde(skip_serializing_if = "Selected::is_absent")]
            note: Selected<Option<String>>,
        }

        let partial = Row {
            name: Selected::Set("a".to_owned()),
            note: Selected::Absent,
        };
        assert_eq!(serde_json::to_string(&partial).unwrap(), r#"{"name":"a"}"#);

        let cleared = Row {
            name: Selected::Set("a".to_owned()),
            note: Selected::Set(None),
        };
        assert_eq!(
            serde_json::to_string(&cleared).unwrap(),
            r#"{"name":"a","note":null}"#
        );
    }
}
