//! Marker traits for a column's declared operators: an undeclared one has no method, so the mistake is compile-time.

use crate::Column;

macro_rules! capability {
    ($($name:ident => $operator:literal),* $(,)?) => {
        $(
            #[doc = concat!("The column declared `", $operator, "` in its `query_ops`.")]
            pub trait $name<C: Column> {}
        )*
    };
}

capability! {
    HasEq => "eq",
    HasNeq => "neq",
    HasGt => "gt",
    HasGte => "gte",
    HasLt => "lt",
    HasLte => "lte",
    HasLike => "like",
    HasILike => "ilike",
    HasNotLike => "not_like",
    HasNotILike => "not_ilike",
    HasAny => "any",
    HasNotAny => "not_any",
    HasContains => "contains",
    HasNotContains => "not_contains",
    HasOverlaps => "overlaps",
    HasNotOverlaps => "not_overlaps",
    HasContainsAll => "contains_all",
    HasNotContainsAll => "not_contains_all",
    HasBetween => "between",
    HasNotBetween => "not_between",
}

/// The column declared `#[db(orderable)]`, so a query may order by it.
pub trait HasOrder<C: Column> {}
