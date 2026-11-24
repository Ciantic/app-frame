# Selecting columns

A projection: which columns a query returns, and how a row is decoded when it
carries only some of them. Both halves are implemented: the builder takes a
selection, and `Query::query` runs the statement and decodes it into a
`{Model}Select` whose fields say whether the column was selected.

The worked examples assume the `orders` model from
[query-builder.md](query-builder.md#worked-examples).

## Goal

```rust
let c = Order::columns();

let query = Order::query()
    .select(&[c.id.column(), c.total.column()])
    .filters([c.status.eq("open")]);

// SELECT "id", "total" FROM "orders" WHERE "status" = $1
let rendered = query.render()?;

let rows: Vec<OrderSelect> = query.build()?.fetch_all(&pool).await?;

assert!(rows[0].id.is_set());
assert!(rows[0].tags.is_absent());
```

## Input side

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

A projection cannot decode into the row type: `#[derive(sqlx::FromRow)]` reads
every field by name and fails on the first one the row does not carry. The row
type also cannot express "the caller did not ask for this", and `Option` would
blur it with "the column is `NULL`".

`{Model}Select` holds one `Selected<T>` field per column, mirroring how
`Patch<T>` distinguishes "not given" on the way in:

```rust
pub enum Selected<T> {
    Absent,
    Set(T),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OrderSelect {
    #[serde(default, skip_serializing_if = "Selected::is_absent")]
    pub status: Selected<String>,
    #[serde(default, skip_serializing_if = "Selected::is_absent")]
    pub total: Selected<i64>,
    /// `Option` stays inside: `Set(None)` is a selected `NULL`.
    #[serde(default, skip_serializing_if = "Selected::is_absent")]
    pub deleted_at: Selected<Option<DateTime<Utc>>>,
}
```

Decoding needs no extra input, because the row itself knows its projection:

```rust
pub fn selected<'r, T>(
    row: &'r PgRow,
    column: &str,
) -> Result<Selected<T>, sqlx::Error>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    if !row.columns().iter().any(|present| present.name() == column) {
        return Ok(Selected::Absent);
    }

    row.try_get::<T, _>(column).map(Selected::Set)
}
```

Verified against PostgreSQL 18 through `Query::query`:

```text
select status, total    -> { status: Set("open"), total: Set(100),
                             id: Absent, tags: Absent }
select *                -> { …, id: Set(…), deleted_at: Set(None) }
```

That is the distinction the design is for: a column the caller did not ask for
and a column the caller asked for that happens to be `NULL` are different
answers, and the JSON response differs too.

- `Selected<T>` lives in `dbkit` beside `Patch<T>`, with `is_absent`, `is_set`,
  `as_set`, `into_set` and `map`.
- It is both `Serialize` and `Deserialize`. `Set` writes the value and `Absent`
  writes `null`; a missing key deserializes to `Absent` through
  `#[serde(default)]` on the field, and `null` deserializes to `Set(None)` for a
  nullable column — a `NULL` that was selected — while a non-nullable column
  rejects it. So a projected row round-trips.
- `Serialize` skips `Absent` on the generated fields, matching the "empty values
  are omitted" rule.
- The struct is generated by `#[derive(DbSelect)]`, one type per model, the way
  `DbPatch` generates the patch; it also implements `Selectable`, whose
  associated `Select` type is what bounds the convenience query.
- `SelectRow::is_empty` reports whether every column is absent, so a nested
  `#[db(flatten)]` select skips itself instead of writing `{}`.
- The scan of `row.columns()` per field is `O(fields × columns)`, which is
  nothing for a table of realistic width; if it ever matters, `from_row` can
  build a `HashSet` once.
- Nothing checks that the projection matches the decoded fields. A select
  struct decoded from a wider row fills more fields than the query asked for,
  which is harmless; from a narrower one it reports `Absent`, which is the
  point.
- A `#[db(flatten)]` field contributes a nested `{Fragment}Select`, so
  `row.size.width` works, the same way the handle struct nests.

### Rendering and building

Two methods, one rendering. `render()` stops at the pair, `build()` takes it to
a `QueryAs`:

```rust
impl<C: Column> Query<C> {
    pub fn render(&self) -> Result<Rendered, Error>;
}

impl<C: Column> Query<C>
where
    C::Model: Selectable,
{
    pub fn build(&self) -> Result<SelectedQuery<C>, Error>;
}
```

`SelectedQuery<C>` is `QueryAs<'static, Postgres, <C::Model as
Selectable>::Select, PgArguments>`. Both the SQL and the arguments are owned,
because `AssertSqlSafe<String>` becomes `SqlStr::Repr::Owned`, so the value can
be held and awaited later rather than only in the same expression:

```rust
let rows: Vec<OrderSelect> = Order::query()
    .select(&[c.id.column(), c.total.column()])
    .filters([c.status.eq("open")])
    .limit(20)
    .build()?
    .fetch_all(&pool)
    .await?;

let query = Order::query().filters([c.vip.eq(true)]).build()?;
let row: OrderSelect = query.fetch_one(&mut *transaction).await?;
```

- The bound is `Selectable`, not `Model`: a model without `#[derive(DbSelect)]`
  renders SQL but has nothing to decode into.
- `fetch_all`, `fetch_one` and `fetch_optional` are sqlx's, so `RowNotFound` and
  counting behave as everywhere else. This replaces three `dbkit` wrappers —
  `query`, `query_one`, `query_optional` — which were one line each over
  exactly these calls; chaining is the smaller surface and the more familiar
  one.
- A render error surfaces from `build()` before any SQL runs, so an empty
  selection or an over-large `limit` is a `?` away.
- `QueryAs` is not `Debug`, so `build().expect_err(..)` does not compile; `?`
  and `unwrap()` on success do.
- `fetch_*` takes a pool, a `&mut Transaction` and a `&mut PgConnection` alike.

Verified: the pool, `&mut *transaction` and `&mut *connection` all run the same
query; a query held between `build()` and `fetch_one` still runs; `fetch_one`
reports `RowNotFound` and `fetch_optional` returns `None`.

### Still open

- Should `DbSelect` also generate the projection, so `OrderSelect::columns()`
  is what `select` takes? It would keep the two sides in step, at the cost of
  coupling them.
- Does a response want `Absent` omitted or written as `null`? Omitted is the
  current answer and matches the "empty values are omitted" rule, but a client
  cannot then tell "not asked for" from "asked for but missing".
- Is a generated select struct right for a narrow endpoint, or is a
  hand-written projection type with three fields better? The generated one is
  mechanical and always in step with the model.
- Should `build()` also expose the SQL it ran, for logging? It discards the
  text after handing it to sqlx; a caller that wants it calls `render()` and
  runs `query_with` itself, which is a second code path to keep in step.

## Where the code lives

- `crates/dbkit/src/query/filter.rs` — `IntoColumns` and its impls, next to
  `ColumnRef` so the handle impl sits with the type it reduces; the borrowed
  forms sit beside the owned ones.
- `crates/dbkit/src/query/mod.rs` — `select(impl IntoColumns<C>)`,
  `Error::EmptySelection`, `render`, `build` and the `SelectedQuery` alias.
- `crates/dbkit/src/query/render.rs` — an empty selection is an error instead of
  `*`.
- `crates/dbkit/src/query/serde.rs` — the `select` key, parsed name by name so
  the visitor stays on `C: Column` without a serde bound on the enum.
- `crates/dbkit/src/select.rs` — `Selected<T>`, `SelectRow`, and the
  `selected(row, column)` decoder.
- `crates/dbkit/src/model.rs` — `Selectable`, the bound the convenience query
  needs.
- `crates/dbkit-macros/src/derive_select.rs` — `#[derive(DbSelect)]`.

## Tests

`crates/dbkit/tests/query.rs`, no database:

- Each call shape builds the documented SQL, and `[Total, Id]` renders in its
  own order.
- `select(Columns::all())` and no `select` both return every column, the first
  as a list and the second as `*`.
- `select(a).select(b)` renders only `b`.
- An empty selection is `Error::EmptySelection`, from Rust and from
  `{"select": []}`.
- `{"select": ["id", "total"]}` parses and round-trips in request order;
  `["total", "id"]` parses to its own query, prints as given, and is not equal
  to the `["id", "total"]` query.
- An unknown selected name reports the column list, a repeated name is one
  column, and a `select` that is not a list is a syntax error.

`crates/dbkit/tests/select.rs`, no database:

- Every column becomes a `Selected` field, and `Absent` fields are omitted from
  the JSON.
- A selected `NULL` writes `null` and is distinct from an unselected column,
  which writes nothing.
- A row round-trips: a missing key is `Absent`, `null` on a nullable column is
  `Set(None)`, and `null` on a non-nullable column is rejected.
- An empty object decodes to a row whose `SelectRow::is_empty` is true.
- A `#[db(flatten)]` field nests its own select struct, and skips itself when
  none of its columns are selected.

Against `pgtest-server`, in `crates/dbkit/tests/query_live.rs`:

- A sparse selection returns the requested columns, and asking a row for a
  column it lacks reports `ColumnNotFound`.
- `build()?.fetch_all` decodes the projected rows: an unselected column is `Absent`,
  and `SELECT *` sets every field with `Set(None)` for a `NULL` column.
- the same chain on a `&mut *transaction` and a `&mut *connection` runs the
  same statement.
- `fetch_one` reports `RowNotFound` and `fetch_optional` returns `None`.
- A render error surfaces from `build()` before any SQL runs.
