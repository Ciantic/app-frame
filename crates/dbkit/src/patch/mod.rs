//! Update inputs: the [`Patch`] value and the builder that applies a batch of them.

mod query;

pub use query::Assignment;
pub use query::PatchError;
pub use query::PatchQuery;
pub use query::PatchRow;
pub use query::RenderedPatch;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

/// One update field: `Absent` (not given), `Null` (clear it) or `Set` (a value); `garde` validates only `Set`.
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
}

impl<T: garde::Validate> garde::Validate for Patch<T> {
    type Context = T::Context;

    fn validate_into(
        &self,
        context: &Self::Context,
        parent: &mut dyn FnMut() -> garde::Path,
        report: &mut garde::Report,
    ) {
        if let Patch::Set(value) = self {
            value.validate_into(context, parent, report);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use garde::Validate;

    #[derive(Debug, Serialize, Deserialize)]
    struct Sample {
        #[serde(default, skip_serializing_if = "Patch::is_absent")]
        name: Patch<String>,
    }

    /// A newtype whose rule is stricter than any primitive's.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, garde::Validate)]
    #[serde(transparent)]
    #[garde(transparent)]
    struct Name(#[garde(length(min = 3))] String);

    #[derive(Debug, Serialize, Deserialize, garde::Validate)]
    struct PatchOfName {
        #[garde(dive)]
        name: Patch<Name>,
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
    fn absent_and_null_pass_validation() {
        for name in [Patch::Absent, Patch::Null] {
            let patch = PatchOfName { name };

            assert!(patch.validate().is_ok());
        }
    }

    #[test]
    fn set_is_validated_by_the_payload_type() {
        let valid = PatchOfName {
            name: Patch::Set(Name("Thingie".to_owned())),
        };
        assert!(valid.validate().is_ok());

        let invalid = PatchOfName {
            name: Patch::Set(Name("no".to_owned())),
        };
        let report = invalid.validate().unwrap_err();
        assert!(report.to_string().starts_with("name"), "{report}");
    }
}
