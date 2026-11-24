//! One field of a projected row, distinguishing "not selected" from "selected `NULL`".

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

/// One column of a projection: `Set` when selected, `Absent` when not; a selected `NULL` stays distinct.
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
            // Unreachable behind `skip_serializing_if`; the only honest value if serialized anyway.
            Selected::Absent => serializer.serialize_none(),
            Selected::Set(value) => value.serialize(serializer),
        }
    }
}

/// A present value is `Set`; a missing key is `Absent` via `#[serde(default)]`, and `null` is a selected `NULL`.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Selected<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Selected::Set)
    }
}

/// A projected row; empty when every column is absent, so a flattened fragment with no selection writes nothing.
pub trait SelectRow {
    fn is_empty(&self) -> bool;
}

/// One column of a row by name; a column absent from the result decodes to `Selected::Absent` instead of failing.
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
