//! The JSON form: a `filters` object, and the paging keys beside it.
//!
//! Inside `filters` a key is a column name with an optional `__operator`
//! suffix, so one object is a level and its keys combine with `AND`. A group is
//! the reserved key `and` or `or` holding an array of objects, one per child,
//! which is what keeps any finite tree expressible without a positional syntax.
//!
//! `filters` is a wrapper rather than the whole payload because paging is not a
//! filter. Keeping the two apart means the top level has no columns at all, so
//! a column named `limit` or `filters` is just a column, and no reserved key
//! has to outrank one.

use crate::Column;
use crate::FilterOperator;
use crate::query::Combinator;
use crate::query::Error;
use crate::query::Filter;
use crate::query::Leaf;
use crate::query::MAX_LIST;
use crate::query::Operand;
use crate::query::Query;
use crate::query::value;
use serde::Deserialize;
use serde::Serialize;
use serde::de::Error as _;
use serde::de::MapAccess;
use serde::de::Visitor;
use serde::ser::Error as _;
use serde::ser::SerializeMap;
use serde_json::Value;
use std::marker::PhantomData;
use std::sync::Arc;

impl<C: Column> Serialize for Query<C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;

        if !self.filters.is_empty() {
            map.serialize_entry("filters", &Level(&self.filters))?;
        }
        if let Some(limit) = self.limit {
            map.serialize_entry("limit", &limit)?;
        }
        if let Some(offset) = self.offset {
            map.serialize_entry("offset", &offset)?;
        }

        map.end()
    }
}

/// One filter object: the children of an implicit conjunction, written as flat
/// keys, falling back to an explicit `and` list when two children would need
/// the same key.
struct Level<'a, C: Column>(&'a [Filter<C>]);

impl<C: Column> Serialize for Level<'_, C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut keys = Vec::with_capacity(self.0.len());
        let mut collision = false;

        for filter in self.0 {
            let key = match filter {
                Filter::Leaf(leaf) => leaf_key(leaf),
                Filter::Group(group) => {
                    reserved::<C>(group.combinator).map_err(S::Error::custom)?;
                    group.combinator.as_str().to_owned()
                }
            };

            collision |= keys.contains(&key);
            keys.push(key);
        }

        let mut map = serializer.serialize_map(Some(if collision { 1 } else { keys.len() }))?;

        if collision {
            map.serialize_entry("and", &Children(self.0))?;

            return map.end();
        }

        for (filter, key) in self.0.iter().zip(&keys) {
            match filter {
                Filter::Leaf(leaf) => map.serialize_entry(key, &leaf_value(leaf))?,
                Filter::Group(group) => map.serialize_entry(key, &Children(&group.filters))?,
            }
        }

        map.end()
    }
}

/// A combinator key is unreachable when the model has a column of that name,
/// because the column wins during deserialization; the query has no JSON form
/// then.
fn reserved<C: Column>(combinator: Combinator) -> Result<(), Error> {
    if C::from_name(combinator.as_str()).is_some() {
        return Err(Error::CombinatorShadowed { combinator });
    }

    Ok(())
}

/// One level's children, written as an array with one object per child.
struct Children<'a, C: Column>(&'a [Filter<C>]);

impl<C: Column> Serialize for Children<'_, C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;

        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;

        for filter in self.0 {
            sequence.serialize_element(&Child(filter))?;
        }

        sequence.end()
    }
}

/// One filter as its own object, which is what an `and`/`or` array holds.
struct Child<'a, C: Column>(&'a Filter<C>);

impl<C: Column> Serialize for Child<'_, C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;

        match self.0 {
            Filter::Leaf(leaf) => map.serialize_entry(&leaf_key(leaf), &leaf_value(leaf))?,
            Filter::Group(group) => {
                reserved::<C>(group.combinator).map_err(S::Error::custom)?;
                map.serialize_entry(group.combinator.as_str(), &Children(&group.filters))?;
            }
        }

        map.end()
    }
}

/// The key a leaf is written under: the bare column for `eq` and `is_null`,
/// `column__operator` otherwise.
fn leaf_key<C: Column>(leaf: &Leaf<C>) -> String {
    let name = leaf.column.name();

    match leaf.operator {
        FilterOperator::Eq | FilterOperator::IsNull => name.to_owned(),
        // `IS NOT NULL` has a shorter spelling, and it round-trips.
        FilterOperator::IsNotNull => format!("{name}__neq"),
        operator => format!("{name}__{}", operator.as_str()),
    }
}

fn leaf_value<C: Column>(leaf: &Leaf<C>) -> Value {
    match &leaf.operand {
        Operand::None => Value::Null,
        Operand::One(value) => value.as_json(),
        Operand::Two(low, high) => Value::Array(vec![low.as_json(), high.as_json()]),
        Operand::List { values, .. } => values.as_json(),
    }
}

impl<'de, C: Column> Deserialize<'de> for Query<C> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Parsed::<C>::deserialize(deserializer)? {
            Parsed(Ok(query)) => Ok(query),
            Parsed(Err(error)) => Err(D::Error::custom(error)),
        }
    }
}

/// A request as the visitor reports it: a syntax error is the deserializer's,
/// and a rejected request is [`Error`].
///
/// `Query::parse` reads this form so a caller keeps the variant it needs to
/// choose an HTTP status; the `Deserialize` impl above flattens both into a
/// `serde_json::Error` for a handler that only wants the message.
pub(crate) struct Parsed<C: Column>(Result<Query<C>, Error>);

/// Reads the JSON form, keeping a rejected request's own variant.
pub(crate) fn parse<C: Column>(json: &str) -> Result<Query<C>, Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);

    match Parsed::<C>::deserialize(&mut deserializer) {
        Ok(parsed) => parsed.0,
        Err(error) => Err(Error::Json(error)),
    }
}

impl<'de, C: Column> Deserialize<'de> for Parsed<C> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_map(QueryVisitor(PhantomData))
            .map(Parsed)
    }
}

struct QueryVisitor<C: Column>(PhantomData<C>);

impl<'de, C: Column> Visitor<'de> for QueryVisitor<C> {
    type Value = Result<Query<C>, Error>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a query object, holding `filters`, `limit` and `offset`")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut query = Query::new();
        let mut seen: Vec<String> = Vec::new();

        // The map is drained even after a rejection: leaving it half-read
        // would turn the first problem into a "trailing characters" syntax
        // error.
        let mut rejected = None;

        while let Some(key) = map.next_key::<String>()? {
            if rejected.is_some() {
                map.next_value::<serde::de::IgnoredAny>()?;

                continue;
            }

            if seen.contains(&key) {
                rejected = Some(Error::DuplicateKey { key });
                map.next_value::<serde::de::IgnoredAny>()?;

                continue;
            }
            seen.push(key.clone());

            match key.as_str() {
                "filters" => match map.next_value::<ParsedLevel<C>>()? {
                    ParsedLevel(Ok(LevelOwned(filters))) => query.filters = filters,
                    ParsedLevel(Err(error)) => rejected = Some(error),
                },
                "limit" => query.limit = Some(map.next_value()?),
                "offset" => query.offset = Some(map.next_value()?),
                _ => {
                    rejected = Some(Error::UnknownKey { key });
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        Ok(match rejected {
            Some(error) => Err(error),
            None => Ok(query),
        })
    }
}

/// One filter object, for the `filters` value and for a level inside an
/// `and`/`or` array.
struct LevelOwned<C: Column>(Vec<Filter<C>>);

struct ParsedLevel<C: Column>(Result<LevelOwned<C>, Error>);

impl<'de, C: Column> Deserialize<'de> for ParsedLevel<C> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_map(LevelVisitor(PhantomData))
            .map(ParsedLevel)
    }
}

struct LevelVisitor<C: Column>(PhantomData<C>);

impl<'de, C: Column> Visitor<'de> for LevelVisitor<C> {
    type Value = Result<LevelOwned<C>, Error>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a filters object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut filters = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut rejected = None;

        while let Some(key) = map.next_key::<String>()? {
            if rejected.is_some() {
                map.next_value::<serde::de::IgnoredAny>()?;

                continue;
            }

            if seen.contains(&key) {
                rejected = Some(Error::DuplicateKey { key });
                map.next_value::<serde::de::IgnoredAny>()?;

                continue;
            }
            seen.push(key.clone());

            let classified = match classify::<C>(&key) {
                Ok(classified) => classified,
                Err(error) => {
                    rejected = Some(error);
                    map.next_value::<serde::de::IgnoredAny>()?;

                    continue;
                }
            };

            match classified {
                Key::Group(combinator) => {
                    let levels = map
                        .next_value::<Vec<ParsedLevel<C>>>()?
                        .into_iter()
                        .map(|parsed| parsed.0)
                        .collect();

                    rejected = children(combinator, levels, &mut filters).err();
                }
                Key::Leaf {
                    column,
                    operator,
                    suffixed,
                } => {
                    let value = map.next_value::<Value>()?;

                    match leaf(column, operator, suffixed, value) {
                        Ok(filter) => filters.push(filter),
                        Err(error) => rejected = Some(error),
                    }
                }
            }
        }

        Ok(match rejected {
            Some(error) => Err(error),
            None => Ok(LevelOwned(filters)),
        })
    }
}

/// What a key inside a filter object means, resolved in the order the grammar
/// states: the whole key as a column, then a combinator, then the last-`__`
/// split.
enum Key<C: Column> {
    Group(Combinator),
    Leaf {
        column: C,
        operator: FilterOperator,
        /// Whether the key spelled the operator out, which decides whether
        /// `null` may stand for `is_null`.
        suffixed: bool,
    },
}

fn classify<C: Column>(key: &str) -> Result<Key<C>, Error> {
    if let Some(column) = C::from_name(key) {
        return Ok(Key::Leaf {
            column,
            operator: FilterOperator::Eq,
            suffixed: false,
        });
    }

    match key {
        "and" => return Ok(Key::Group(Combinator::And)),
        "or" => return Ok(Key::Group(Combinator::Or)),
        _ => {}
    }

    let Some((name, suffix)) = key.rsplit_once("__") else {
        return Err(Error::UnknownColumn {
            name: key.to_owned(),
            known: names::<C>(),
        });
    };

    let Some(column) = C::from_name(name) else {
        return Err(Error::UnknownColumn {
            name: name.to_owned(),
            known: names::<C>(),
        });
    };

    let Some(operator) = FilterOperator::from_name(suffix) else {
        return Err(Error::UnknownOperator {
            name: suffix.to_owned(),
        });
    };

    Ok(Key::Leaf {
        column,
        operator,
        suffixed: true,
    })
}

fn names<C: Column>() -> Vec<&'static str> {
    C::all().iter().map(|column| column.name()).collect()
}

/// Appends one `and`/`or` key to the level being read.
///
/// `and` is the level's own combinator, so its children join the level instead
/// of nesting: that is what makes the serializer's `{"and": [...]}` fallback
/// parse back to the tree it came from.
fn children<C: Column>(
    combinator: Combinator,
    levels: Vec<Result<LevelOwned<C>, Error>>,
    level: &mut Vec<Filter<C>>,
) -> Result<(), Error> {
    let mut children = Vec::with_capacity(levels.len());

    for parsed in levels {
        let LevelOwned(filters) = parsed?;

        match filters.len() {
            0 => return Err(Error::EmptyGroup { combinator }),
            1 => children.extend(filters),
            _ => children.push(Filter::group(Combinator::And, filters)),
        }
    }

    if children.is_empty() {
        return Err(Error::EmptyGroup { combinator });
    }

    match combinator {
        Combinator::And => level.extend(children),
        Combinator::Or => level.push(Filter::group(Combinator::Or, children)),
    }

    Ok(())
}

fn leaf<C: Column>(
    column: C,
    operator: FilterOperator,
    suffixed: bool,
    value: Value,
) -> Result<Filter<C>, Error> {
    let name = column.name();
    let pg_type = column.pg_type();

    if !operator.is_nullary() && !column.query_ops().contains(&operator) {
        return Err(Error::OperatorNotAllowed {
            column: name,
            operator,
            allowed: column.query_ops(),
        });
    }

    let operand = match operator {
        FilterOperator::IsNull | FilterOperator::IsNotNull => {
            if value != Value::Bool(true) {
                return Err(Error::NotTrue { operator });
            }

            Operand::None
        }
        // A bare key with `null` is the short form of `is_null`; spelling out
        // `eq` is not, so a cleared column keeps one spelling.
        FilterOperator::Eq if value.is_null() => {
            if suffixed {
                return Err(Error::NullNotAllowed { column: name });
            }

            return Ok(Filter::leaf(column, FilterOperator::IsNull, Operand::None));
        }
        FilterOperator::Neq if value.is_null() => {
            return Ok(Filter::leaf(
                column,
                FilterOperator::IsNotNull,
                Operand::None,
            ));
        }
        FilterOperator::Between | FilterOperator::NotBetween => {
            let Some(items) = value.as_array() else {
                return Err(Error::WrongShape {
                    operator,
                    expected: "two values",
                    actual: value::describe(&value).to_owned(),
                });
            };
            if items.len() != 2 {
                return Err(Error::WrongShape {
                    operator,
                    expected: "two values",
                    actual: format!("{} values", items.len()),
                });
            }

            let low = Arc::from(value::scalar(pg_type, name, &items[0])?);
            let high = Arc::from(value::scalar(pg_type, name, &items[1])?);

            Operand::Two(low, high)
        }
        FilterOperator::In | FilterOperator::NotIn => {
            if value::is_array(pg_type) {
                return Err(Error::ArrayList { column: name });
            }

            list(operator, pg_type, name, &value)?
        }
        FilterOperator::Overlaps
        | FilterOperator::NotOverlaps
        | FilterOperator::ContainsAll
        | FilterOperator::NotContainsAll => {
            if !value::is_array(pg_type) {
                return Err(Error::ArrayRequired {
                    column: name,
                    operator,
                });
            }

            list(operator, value::element_pg_type(pg_type), name, &value)?
        }
        FilterOperator::Contains | FilterOperator::NotContains => {
            if !value::is_array(pg_type) {
                return Err(Error::ArrayRequired {
                    column: name,
                    operator,
                });
            }

            let element = value::element_pg_type(pg_type);
            let element = value::scalar(element, name, &value)?;

            Operand::One(Arc::from(element))
        }
        _ => Operand::One(Arc::from(value::scalar(pg_type, name, &value)?)),
    };

    Ok(Filter::leaf(column, operator, operand))
}

fn list(
    operator: FilterOperator,
    element_pg_type: &'static str,
    column: &'static str,
    value: &Value,
) -> Result<Operand, Error> {
    let (values, len) = value::list(operator, element_pg_type, column, value)?;

    if len == 0 {
        return Err(Error::EmptyList { operator });
    }
    if len > MAX_LIST {
        return Err(Error::ListTooLong { operator, len });
    }

    Ok(Operand::List {
        values: Arc::from(values),
        len,
    })
}
