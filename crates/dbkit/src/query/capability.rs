//! Which operators a column declared, as empty marker traits.
//!
//! `#[derive(DbModel)]` writes one empty impl per `#[db(query_ops(...))]` entry
//! for the column's tag type, and every operator method of
//! [`ColumnRef`](crate::ColumnRef) is gated on the matching trait here. An
//! operator the column never declared therefore does not resolve at all, so
//! the mistake is a compile error rather than a rejected query.
//!
//! `is_null` and `is_not_null` have no trait: they take no value and cannot be
//! mistyped, so every column can use them.

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
    HasIn => "in",
    HasNotIn => "not_in",
    HasContains => "contains",
    HasNotContains => "not_contains",
    HasOverlaps => "overlaps",
    HasNotOverlaps => "not_overlaps",
    HasContainsAll => "contains_all",
    HasNotContainsAll => "not_contains_all",
    HasBetween => "between",
    HasNotBetween => "not_between",
}
