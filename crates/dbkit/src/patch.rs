use serde::Deserialize;
use serde::Deserializer;

/// One field of an update, distinguishing "not given" from "set to `NULL`".
///
/// A patch is an input type: it deserializes, it does not serialize. That is
/// what keeps `null` from being propagated over the wire — `Patch::Null` only
/// travels from the client to the repository to express "clear this nullable
/// column", and a model never echoes it back.
///
/// With `#[serde(default)]` on the field, a missing key is [`Patch::Absent`],
/// an explicit `null` is [`Patch::Null`] and any other value is
/// [`Patch::Set`]. No custom deserializers are needed on the patch struct.
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

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Patch::Set(value),
            None => Patch::Null,
        })
    }
}

/// Restates the rule at the type level: a patch never serializes, so
/// `Patch::Null` cannot become a `null` in a response.
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Sample {
        #[serde(default)]
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
}
