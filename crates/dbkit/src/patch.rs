use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

/// One field of an update, distinguishing "not given" from "set to `NULL`".
///
/// `#[serde(default)]` maps a missing key to [`Patch::Absent`] and an explicit
/// `null` to [`Patch::Null`](Patch::Null), so a JSON body spells all three
/// states without custom deserializers. Fields are serialized with
/// `skip_serializing_if = "Patch::is_absent"`, which also keeps `Absent` off
/// the wire.
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

    #[derive(Debug, Serialize, Deserialize)]
    struct Sample {
        #[serde(default, skip_serializing_if = "Patch::is_absent")]
        name: Patch<String>,
    }

    #[test]
    fn absent_is_the_default() {
        let sample: Sample = serde_json::from_str("{}").unwrap();

        assert!(sample.name.is_absent());
        assert_eq!(serde_json::to_string(&sample).unwrap(), "{}");
    }

    #[test]
    fn reads_set_and_null() {
        let set: Sample = serde_json::from_str(r#"{"name":"Thingie"}"#).unwrap();
        let null: Sample = serde_json::from_str(r#"{"name":null}"#).unwrap();

        assert_eq!(set.name, Patch::Set("Thingie".to_owned()));
        assert_eq!(null.name, Patch::Null);
    }

    #[test]
    fn writes_set_and_null() {
        let set = Sample {
            name: Patch::Set("Thingie".to_owned()),
        };
        let null = Sample { name: Patch::Null };

        assert_eq!(
            serde_json::to_string(&set).unwrap(),
            r#"{"name":"Thingie"}"#
        );
        assert_eq!(serde_json::to_string(&null).unwrap(), r#"{"name":null}"#);
    }
}
