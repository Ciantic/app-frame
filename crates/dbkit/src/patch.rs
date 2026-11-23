use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::fmt::Debug;
use std::marker::PhantomData;
use zod_rs::__private::ValidationResult;
use zod_rs::prelude::Schema;
use zod_rs::prelude::Value;

/// One field of an update, distinguishing "not given" from "set to `NULL`".
///
/// With `#[serde(default)]` on the field, a missing key is [`Patch::Absent`],
/// an explicit `null` is [`Patch::Null`] and any other value is
/// [`Patch::Set`]. No custom deserializers are needed on the patch struct.
///
/// A patch serializes for responses: `Set` writes the value and `Null` writes
/// `null`. `Absent` also writes `null`, so a returned struct needs
/// `skip_serializing_if = "Patch::is_absent"` to keep an absent field off the
/// wire.
///
/// [`Patch::schema`] validates one patch value: `null` is [`Patch::Null`] and
/// any other value is validated by the inner schema and becomes
/// [`Patch::Set`]. A missing key has no value to validate, so a patch field in
/// an object schema is an `optional_field`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Patch<T> {
    #[default]
    Absent,
    Set(T),
    Null,
}

impl<T> Patch<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Patch::Absent)
    }

    pub fn is_set(&self) -> bool {
        matches!(self, Patch::Set(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Patch::Null)
    }

    pub fn as_set(&self) -> Option<&T> {
        match self {
            Patch::Set(value) => Some(value),
            _ => None,
        }
    }

    pub fn into_set(self) -> Option<T> {
        match self {
            Patch::Set(value) => Some(value),
            _ => None,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Patch<U> {
        match self {
            Patch::Absent => Patch::Absent,
            Patch::Set(value) => Patch::Set(f(value)),
            Patch::Null => Patch::Null,
        }
    }

    pub fn schema<S>(inner: S) -> PatchSchema<S, T>
    where
        S: Schema<T>,
        T: Debug,
    {
        PatchSchema {
            inner,
            _value: PhantomData,
        }
    }
}

/// Schema of one [`Patch`] value.
#[derive(Debug)]
pub struct PatchSchema<S, T> {
    inner: S,
    _value: PhantomData<fn() -> T>,
}

impl<S, T> Schema<Patch<T>> for PatchSchema<S, T>
where
    S: Schema<T>,
    T: Debug,
{
    fn validate(&self, value: &Value) -> Result<Patch<T>, ValidationResult> {
        if value.is_null() {
            return Ok(Patch::Null);
        }
        self.inner.validate(value).map(Patch::Set)
    }
}

impl<T: Serialize> Serialize for Patch<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Patch::Absent | Patch::Null => serializer.serialize_none(),
            Patch::Set(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Patch::Set(value),
            None => Patch::Null,
        })
    }
}

/// Restates the rules at the type level: a patch writes `null` for both
/// `Absent` and `Null` unless the field skips absent values, and its schema
/// accepts `null` as `Null` and validates everything else.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zod_rs::prelude::string;

    #[derive(Debug, Serialize, Deserialize)]
    struct Sample {
        #[serde(default, skip_serializing_if = "Patch::is_absent")]
        name: Patch<String>,
    }

    #[test]
    fn missing_key_is_absent() {
        let sample: Sample = serde_json::from_str("{}").unwrap();

        assert!(sample.name.is_absent());
    }

    #[test]
    fn null_is_null() {
        let sample: Sample = serde_json::from_str(r#"{"name":null}"#).unwrap();

        assert!(sample.name.is_null());
    }

    #[test]
    fn value_is_set() {
        let sample: Sample = serde_json::from_str(r#"{"name":"Thingie"}"#).unwrap();

        assert_eq!(sample.name, Patch::Set("Thingie".to_owned()));
    }

    #[test]
    fn absent_is_omitted() {
        let sample = Sample {
            name: Patch::Absent,
        };

        assert_eq!(serde_json::to_string(&sample).unwrap(), "{}");
    }

    #[test]
    fn null_writes_null() {
        let sample = Sample { name: Patch::Null };

        assert_eq!(serde_json::to_string(&sample).unwrap(), r#"{"name":null}"#);
    }

    #[test]
    fn set_writes_value() {
        let sample = Sample {
            name: Patch::Set("Thingie".to_owned()),
        };

        assert_eq!(
            serde_json::to_string(&sample).unwrap(),
            r#"{"name":"Thingie"}"#
        );
    }

    #[test]
    fn schema_accepts_null_as_null() {
        let schema = Patch::<String>::schema(string());

        assert_eq!(schema.validate(&json!(null)).unwrap(), Patch::Null);
    }

    #[test]
    fn schema_validates_set_value() {
        let schema = Patch::<String>::schema(string().min(3));

        assert_eq!(
            schema.validate(&json!("Thingie")).unwrap(),
            Patch::Set("Thingie".to_owned())
        );
        assert!(schema.validate(&json!("no")).is_err());
    }

    #[test]
    fn schema_fits_an_optional_object_field() {
        let schema =
            zod_rs::object().optional_field("name", Patch::<String>::schema(string().min(3)));

        assert!(schema.validate(&json!({})).is_ok());
        assert!(schema.validate(&json!({"name": null})).is_ok());
        assert!(schema.validate(&json!({"name": "Thingie"})).is_ok());
        assert!(schema.validate(&json!({"name": "no"})).is_err());
    }
}
