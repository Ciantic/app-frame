# Selecting columns

A projection: which columns a query returns, and how a row is decoded when it
carries only some of them. The two halves are independent; the input side is
implemented, the output side is still a sketch.

The worked examples assume the `orders` model from
[query-builder.md](query-builder.md#worked-examples).

## Goal

```rust
let c = Order::columns();

let query = Order::query()
    .select(&[c.id.column(), c.total.column()])
    .filters([c.status.eq("open")]);

let built = query.build()?;
// SELECT "id", "total" FROM "orders" WHERE "status" = $1
```

and, once the output type exists:

```rust
let row: OrderSelect =
    sqlx::query_as_with(AssertSqlSafe(built.sql), built.arguments)
        .fetch_one(&pool)
        .await?;

assert!(row.id.is_set());
assert!(row.tags.is_absent());
```

## Input side: implemented

`IntoColumns`, `Query::select`, `Error::EmptySelection` and the `select` JSON
key are in `dbkit`. What follows is the reasoning behind them.

- `Columns<C>` is an `IndexSet<C>`: it deserializes from an array of column
  names, rejects a name that is not a column, and iterates in the order the
  columns were added. It is the selection set, and the JSON form falls out of
  it.
- `Query::select` takes `Option<Columns<C>>` and renders `SELECT *` for `None`
  and the quoted list otherwise.
- `ColumnRef<Tag, C, T>::column()` returns the bare `C`, which is the escape
  from the per-column handle type.

So the SQL side works; what is missing is an ergonomic way to build the set
from handles, and any way to decode a sparse row.

## Passing the columns

The obstacle is that handles are one type per column: `c.id` is
`ColumnRef<OrderIdTag, OrderColumn, OrderId>` and `c.total` is
`ColumnRef<OrderTotalTag, OrderColumn, i64>`, so `[c.id, c.total]` is two
different types and cannot be one array. Filter leaves avoid this because each
operator returns the same `Filter<C>`; a selection needs the same trick, and
the only step that erases `Tag` and `T` is `column()`, which yields `C`.

Verified shapes, all through one trait:

```rust
pub trait IntoColumns<C: Column> {
    fn into_columns(self) -> Columns<C>;
}

// One column, either raw or through its handle.
impl<C: Column> IntoColumns<C> for C {}
impl<Tag, C: Column, T> IntoColumns<C> for ColumnRef<Tag, C, T> {}

// A prebuilt set, which is also how "every column" is spelled.
impl<C: Column> IntoColumns<C> for Columns<C> {}

// A list, owned or borrowed. The elements are one type, so a handle list
// needs `.column()`.
impl<C: Column, const N: usize> IntoColumns<C> for [C; N] {}
impl<C: Column, const N: usize> IntoColumns<C> for &[C; N] {}
impl<C: Column> IntoColumns<C> for &[C] {}
impl<C: Column> IntoColumns<C> for Vec<C> {}
impl<C: Column> IntoColumns<C> for &Vec<C> {}

// And the set by reference, so `select(&columns)` works on a `Columns<C>` the
// caller keeps.
impl<C: Column> IntoColumns<C> for &Columns<C> {}

impl<C: Column> Query<C> {
    pub fn select(self, columns: impl IntoColumns<C>) -> Self;
}
```

The reference impls are not a separate trait or a `Borrow` bound: `Column` is
this crate's trait and none of those reference types implements it, so
coherence proves each one disjoint from `for C` and they coexist with the
owned forms.

What each call yields:

- `select(c.id)` — one column, through its handle.
- `select(OrderColumn::Id)` — one column, spelled as the bare enum variant.
- `select(&[c.id.column(), c.total.column()])` — a list of handles, each reduced
  to its column. A borrow, because the list is the caller's.
- `select(&[OrderColumn::Total, OrderColumn::Id])` — a list of variants, rendered
  in the order given, so `"total", "id"`.
- `select(Columns::all())` — every column.
- `select(columns)` with a `Columns<C>` from a request — that set.

The one ugliness is `.column()` on every element of a multi-column list. The
alternative that removes it — a tuple, `select((c.id, c.total))` — cannot
coexist with the single-column form: a blanket
`impl<A: IntoColumn<C>> IntoColumns<C> for A` overlaps the tuple, array and
`Columns` impls, and coherence rejects it even though none of those types is a
handle. Keeping tuples means dropping `select(c.id)` and writing
`select((c.id,))` for one column. Verified: the two forms cannot coexist, so
choosing tuples costs the trailing comma on a single column.

A `columns![c.id, c.total]` macro would give the best call sites and needs no
trait, but it puts a macro in the API and moves the compiler's diagnostics into
the expansion. The plan keeps it as an option, not the design.

### Order and emptiness

- Order is the caller's. `Columns<C>` is an `IndexSet`, so `[Total, Id]` renders
  `"total", "id"` and a request round-trips byte for byte; `Columns::all()`
  adds the columns in declaration order. Two selections with the same columns in
  different orders are not equal, because they render different SQL — and
  therefore different prepared statements, which is the one cost of keeping the
  order instead of canonicalizing it.
- Calling `select` twice replaces the first selection rather than adding to it.
  Appending would make `select(a).select(b)` silently mean `a ∪ b`, and a
  projection has one selection.
- An empty selection is an error, not `SELECT *`. Today `Some(empty)` renders
  `*`, which turns a mistyped `"select": []` into a full scan of a table.
  `SELECT` with no columns is not valid SQL either, so
  `Error::EmptySelection` at `build()` is the honest answer. `None` stays
  "every column"; a caller that builds the set from a request maps an empty
  list to `None` if it wants that.

## The JSON key

The `filters` wrapper from the previous change made this cheap: the top level
is already a fixed set of keys, so `select` joins it without shadowing a column
of that name.

```json
{
  "select": ["id", "total"],
  "filters": { "status": "open" },
  "limit": 20
}
```

- `Columns<C>` already deserializes from an array of names and rejects an
  unknown one with the column list, so the key needs no new parser.
- `{"select": []}` deserializes to an empty set, which `build()` rejects; it
  must not mean every column.
- Serialization writes `select` first, then `filters`, `limit` and `offset`,
  and omits it when there is no projection.
- Column order is the request's, like filter order: `{"select": ["total", "id"]}`
  prints `{"select": ["total", "id"]}` and renders in that order.

## The output type

Not settled; this is the sketch to build against.

A `SELECT *` into `Order` still works, but a projection cannot decode into the
row type: `#[derive(sqlx::FromRow)]` reads every field by name and fails on the
first one the row does not carry. The row type also cannot express "the caller
did not ask for this", and `Option` would blur it with "the column is `NULL`".

The proposal is a `{Model}Select` struct with one `Selected<T>` field per
column, mirroring how `Patch<T>` distinguishes "not given":

```rust
pub enum Selected<T> {
    Absent,
    Set(T),
}

#[derive(Debug, Serialize)]
pub struct OrderSelect {
    pub status: Selected<String>,
    pub total: Selected<i64>,
    /// `Option` stays inside: `Set(None)` is a selected `NULL`.
    pub deleted_at: Selected<Option<DateTime<Utc>>>,
}
```

Decoding needs no extra input, because the row itself knows its projection:

```rust
fn selected<'r, T>(row: &'r PgRow, name: &str) -> Result<Selected<T>, sqlx::Error>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    if !row.columns().iter().any(|column| column.name() == name) {
        return Ok(Selected::Absent);
    }

    row.try_get::<T, _>(name).map(Selected::Set)
}
```

Verified against PostgreSQL 18 with the `orders` table:

```text
select "name", "total"  -> { id: Absent, name: Set("a"), total: Set(1),
                             deleted_at: Absent }
select *                -> { id: Set(…), name: Set("a"), total: Set(1),
                             deleted_at: Set(None) }
```

That is the distinction the design is for: a column the caller did not ask for
and a column the caller asked for that happens to be `NULL` are different
answers, and the JSON response differs too.

- `Selected<T>` lives in `dbkit` beside `Patch<T>`, with `is_absent`, `is_set`,
  `as_set`, `into_set` and `map`.
- `Serialize` skips `Absent` — `skip_serializing_if = "Selected::is_absent"` on
  the field, matching the "empty values are omitted" rule. It is an output
  type, so there is no `Deserialize`.
- The struct is generated by a derive, `#[derive(DbSelect)]`, one type per
  model, the way `DbPatch` generates the patch. `DbModel` already knows the
  column names, types and nullability.
- The scan of `row.columns()` per field is `O(fields × columns)`, which is
  nothing for a table of realistic width; if it ever matters, `from_row` can
  build a `HashSet` once.
- Nothing checks that the projection matches the decoded fields. A select
  struct decoded from a wider row fills more fields than the query asked for,
  which is harmless; from a narrower one it reports `Absent`, which is the
  point.
- A `#[db(flatten)]` field contributes a nested `{Fragment}Select`, so
  `row.size.width` works, the same way the handle struct nests.

### Open questions for the output side

- Should `DbSelect` also generate the projection? It knows the columns, so
  `OrderSelect::columns()` could be the set to pass to `select`, and the two
  could not disagree. That couples the two halves now kept apart.
- Should an `Absent` field be `null` or omitted in a response? Omitted keeps
  "not asked for" out of the payload, but a client cannot then tell it from an
  unselected column it expected.
- Do we need a select struct at all, or is a per-endpoint projection type
  written by hand better? The generated one is mechanical and always in step
  with the model; a hand-written one is fewer fields for a narrow endpoint.
- Should the row type decode a projection via `Option<T>` fields instead? It
  cannot: `Option` already means `NULL` in SQL, and sqlx's derive would fail
  before that question.
- `SELECT *` into `OrderSelect` sets every field, so a caller that does not
  care about the projection can ignore `select` and still use the same type.

## Where the code lives

Done:

- `crates/dbkit/src/query/filter.rs` — `IntoColumns` and its four impls, next to
  `ColumnRef` so the handle impl sits with the type it reduces.
- `crates/dbkit/src/query/mod.rs` — `select(impl IntoColumns<C>)` and
  `Error::EmptySelection`.
- `crates/dbkit/src/query/render.rs` — an empty selection is an error instead of
  `*`.
- `crates/dbkit/src/query/serde.rs` — the `select` key, parsed name by name so
  the visitor stays on `C: Column` without a serde bound on the enum.

Still to come, for the output side:

- `crates/dbkit/src/select.rs` — `Selected<T>`.
- `crates/dbkit-macros/src/derive_select.rs` — `#[derive(DbSelect)]`, wired into
  `dbkit-macros/src/lib.rs`.

## Tests

- Each call shape above builds the documented SQL, and `[Total, Id]` renders
  its own order. Done, in
  `every_selection_shape_builds_its_columns_in_its_own_order` and a live run of
  the handle form.
- `select(Columns::all())` and no `select` both return every column, the first
  as a list and the second as `*`.
- `select(a).select(b)` renders only `b`. Done.
- An empty selection is `Error::EmptySelection`, from Rust and from
  `{"select": []}`. Done.
- `{"select": ["id", "total"]}` parses, round-trips in request order, and an
  unknown name reports the column list. Done.
- `{"select": ["total", "id"]}` parses to its own query, prints `["total",
  "id"]`, and is not equal to the `["id", "total"]` query. Done.
- Against PostgreSQL: a sparse selection returns the requested columns, and
  asking for a column the row lacks reports `ColumnNotFound`. Done, which is
  the mechanism `OrderSelect` will build on.
- With the select struct: `OrderSelect` reports `Absent` for an unselected
  column; `SELECT *` reports `Set` for every field and `Set(None)` for a `NULL`
  column; a selected `NULL` and an unselected column both survive into the
  response with different JSON.
