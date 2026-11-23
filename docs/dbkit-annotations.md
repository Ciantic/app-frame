# dbkit annotations

Annotations understood by `#[derive(DbModel)]` from the `dbkit-macros` crate.
They describe the fixed columns of a table row: which fields form the primary
key, which one carries the optimistic-locking token, which ones are audit
columns, which columns the database fills in from a default, and which
comparisons each column may be queried with. Every field carries a `pgtype`
unless it is flattened, and a table row needs a primary key; the other
annotations are opt-in.

A struct is one of two shapes:

- A table row, `#[db(table = "...")]`, which generates `impl dbkit::Model`
  with `Model::meta()`.
- A fragment, `#[db(fragment)]`, which is a group of columns with no table of
  its own. It generates `impl dbkit::Model` returning
  `Meta::Fragment(&TableFragment { columns })`, and another model embeds it
  with `#[db(flatten)]`.

`#[derive(DbPatch)]` reads the same annotations to generate the model's patch
input type, which `garde` validates; insert implementations do not exist yet,
so the insert sections describe a contract rather than code you can call today.
The accepted syntax stays as documented here.

## Example

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, DbModel)]
#[db(table = "email")]
pub struct Email {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    #[db(pgdefault = "uuidv7()")]
    pub id: EmailId,

    #[db(pgtype = "text")]
    pub from: String,

    #[db(pgtype = "text")]
    pub to: String,

    #[db(pgtype = "text")]
    #[db(query_ops(eq, neq, like, ilike, in))]
    pub subject: String,

    #[db(pgtype = "text")]
    pub body: String,

    #[db(pgtype = "timestamptz")]
    pub sent_at: Option<DateTime<Utc>>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "int8")]
    #[db(pgdefault = "0")]
    pub version: i64,
}
```

Fields without a role are ordinary business columns. They are the fields a
generated patch would expose; role-annotated fields never are.

## Where annotations may appear

- `#[db(table = "...")]` or `#[db(fragment)]` on the struct, exactly once.
  They are mutually exclusive; `table` makes the struct a table row and
  `fragment` makes it a column group.
- `#[db(pgtype = "...")]` on every field that is not `#[db(flatten)]`, exactly
  once.
- `#[db(pgdefault = "...")]` on a field, at most once.
- `#[db(primary_key)]`, `#[db(version)]`, `#[db(created_at)]`,
  `#[db(updated_at)]` on a field of a table row.
- `#[db(query_ops(...))]` on a field, at most once; the parentheses hold a
  comma-separated operator list.
- `#[db(flatten)]` on a field of a table row, at most once.
- Multiple `#[db(...)]` attributes on one field are allowed; the roles are
  collected across all of them.
- All attributes in the accepted vocabulary are listed here. Anything else is a
  compile error rather than an ignored annotation, so a typo never silently
  changes behaviour.

## Style

The derive accepts a role, `pgtype` and `pgdefault` in one attribute, e.g.
`#[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]`, but models do
not use that form:

- One annotation per line, each in its own `#[db(...)]` attribute.
- Role first, then `pgtype`, then `pgdefault`, then `query_ops`:
  `#[db(primary_key)]` above `#[db(pgtype = "uuid")]` above
  `#[db(pgdefault = "uuidv7()")]` above `#[db(query_ops(eq, in))]`.
- `#[serde(...)]` attributes follow the `#[db(...)]` block.
- A `#[db(flatten)]` field carries that annotation alone.
- A blank line separates fields, so the annotation block reads as one unit per
  column.

The combined form stays supported and is still covered by the derive's tests,
but new and edited models use the one-annotation-per-line layout.

## `table`

Names the table the row belongs to.

- Legal on: the struct, exactly once.
- Value: a non-empty string literal.
- Missing, empty, or repeated `table` is an error.
- The value is the plain identifier without quotes or a schema prefix;
  generated SQL quotes it as `"..."`.
- A table cannot have generic parameters, because the metadata cache is one
  `static` per type.

## `fragment`

Makes the struct a group of columns instead of a table row.

- Legal on: the struct, exactly once, as an alternative to `table`.
- Value: none; it is a bare annotation.
- A fragment has no table name, no primary key and no roles. `pgtype` is
  required on every field, and `pgdefault` and `query_ops` work as usual.
- The derive generates `impl dbkit::Model`, whose `meta()` is
  `Meta::Fragment(&TableFragment { columns })`: no table name, no key, no
  roles to fake. A model embeds it with [`flatten`](#flatten).
- `Meta::Fragment` is what `flatten` matches on: splicing a fragment is fine,
  splicing a table panics on the first `meta()` call with a message naming the
  field.
- A fragment is flat: it cannot itself `#[db(flatten)]` another struct.
- `#[derive(DbPatch)]` on a fragment emits `<Fragment>Patch`, whose fields are
  all `Patch<T>` because a fragment has no key or version to carry.
- Fragments are not listed in the crate's `MODELS` list, because they are not
  tables.

```rust
#[derive(DbModel)]
#[db(fragment)]
pub struct Size {
    #[db(pgtype = "int4")]
    pub width: i32,

    #[db(pgtype = "int4")]
    pub height: i32,
}
```

## `flatten`

Splices the columns of a fragment into the enclosing table.

- Legal on: a field of a table row, at most once.
- Value: none; it is a bare annotation.
- The field type must derive `DbModel` with `#[db(fragment)]`. The derive
  cannot inspect the type, so it matches on `<Type as dbkit::Model>::meta()`
  and, on `Meta::Fragment`, extends the column list with its `columns`. A type
  that does not derive `DbModel` fails to compile; a table panics on the first
  `meta()` call, naming the field and the table it found.
- The fragment's columns take the field's position in the table's column list,
  so declaration order is preserved across plain and flattened fields.
- A flattened field is not a column itself: it must not carry `pgtype`,
  `pgdefault`, `query_ops` or a role, and it must not be `Option`. An optional
  group is expressed by nullable columns inside the fragment.
- The field stays an ordinary struct field on the row type, so reading the row
  and serializing it are unaffected.
- `#[derive(DbPatch)]` emits the field as a `#[serde(flatten)]` field of type
  `<FieldType>Patch`, so the fragment's columns are top-level keys of the patch.
  The fragment must derive `DbPatch` too.
- Column name collisions across fragments are not detected by the derive;
  `check_live_schema_against_models` reports a column declared more than once.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, DbModel)]
#[db(table = "frame")]
pub struct Frame {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: FrameId,

    #[db(flatten)]
    pub size: Size,

    #[db(pgtype = "text")]
    pub label: String,
}
```

The table has columns `id`, `width`, `height` and `label`, in that order.

## `pgtype`

Names the column type, written the way it is written in `schema.sql`.

- Legal on: every field that is not `#[db(flatten)]`, required.
- Value: a non-empty string literal holding a SQL type: `uuid`, `text`,
  `bigint`, `boolean`, `timestamptz`, `jsonb`, `varchar(255)`,
  `numeric(10,2)`, `text[]`, `text[][]`.
- Missing, empty, or repeated `pgtype` is an error.
- Written in its own attribute, below the role attribute; the combined form is
  accepted but not used in models (see [Style](#style)).
- The value is DDL, not a `pg_type` catalog name, so it is what a generated
  `schema.sql` would emit. Include the length, precision and array brackets
  when the column has them.
- Spelling variants that PostgreSQL treats as the same type are equivalent:
  `int8` and `bigint`, `varchar(255)` and `character varying(255)` both compare
  equal. Short forms are the convention here.
- Array dimensions are not part of the type: `text[][]` and `text[]` are the
  same column type, so both annotations pass.
- A domain column must be annotated with the domain name, not its base type,
  because that is the column's type.
- The derive does not resolve the value; the schema test has PostgreSQL parse
  it and compares the resulting type and typmod with the real column. A typo or
  a `varchar(50)` against a `varchar(255)` column fails the test.

## `pgdefault`

Records the column default, written the way it is written in `schema.sql`.

- Legal on: a field, optional and at most once.
- Value: a non-empty string literal holding a default expression: `uuidv7()`,
  `now()`, `0`, `'{}'::jsonb`.
- Omitted when the column has no default. An empty or repeated `pgdefault` is
  an error.
- Meant to mirror the database rather than change it: the model does not emit
  the default. It tells a generated insert that the column can be omitted and
  lets the schema test detect drift.
- A nullable column may carry a default.
- The derive does not resolve the expression; the schema test puts it on a
  probe column and compares PostgreSQL's normalized form (`current_timestamp`
  compares equal to `now()`) with the real column's default. An expression the
  database cannot parse fails the test.
- A defaulted column is still a plain field on the row type: reads always see
  the value the database stored.

## `query_ops`

Declares which comparisons a column may be queried with.

- Legal on: a field, optional and at most once.
- Value: one or more operators inside parentheses, comma-separated:
  `#[db(query_ops(eq, neq, like, in))]`. A bare `#[db(eq)]` is not accepted,
  and an empty `query_ops()` is an error.
- Operators: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `like`, `ilike`, `not_like`,
  `not_ilike`, `in`, `not_in`, `any`, `not_any`, `is_null`, `is_not_null`,
  `between`, `not_between`. They map one-to-one onto
  `dbkit::FilterOperator`.
- Order is preserved and duplicates are an error.
- Metadata only: the derive records the operators in
  `ColumnMeta::query_ops` and nothing else. No SQL is generated, no operator is
  validated against the column type, and the annotation does not change how a
  row is read or written. A query layer is expected to look the list up to
  reject an operator a column does not support.
- Columns without the annotation get an empty list, meaning "not queryable"
  rather than "every operator allowed".
- Nothing stops `is_null` on a non-nullable column or `like` on a `uuid`; those
  are semantic mistakes a future query layer would have to reject, not the
  derive.

## `primary_key`

Marks a primary key column. Repeat it on every column of a composite key.

- Legal on: one or more fields, required.
- A field must not be `Option`.
- Any type is accepted; it must encode to the database column by itself.
- A single annotated field is the `Model::Id` associated type; with several, the
  associated type is a tuple of the field types in declaration order.
- Used for the `WHERE` clause of single-row operations and for `find`.
- A required field of the generated patch.

## `version`

Marks the optimistic-locking field.

- Legal on: at most one field, optional. A model without it gets no concurrency
  guard, so writes update unconditionally; use it for append-only rows and for
  tables where the last writer may win.
- The field must be `i64` and must not be `Option`.
- Reads produce `Model::version()`, writes append the guard
  `"version" = "version" + 1` and the condition `WHERE "version" = $n`.
- A guard miss surfaces as `RepoError::Conflict`, not as a generic database
  error.
- A required field of the generated patch, where the update carries it as the
  optimistic-locking token. A model without it gets a patch without a version.

## `created_at`

Marks the creation timestamp.

- Legal on: at most one field, optional. A model without it gets no creation
  timestamp handling.
- The field must be a `DateTime` and must not be `Option`.
- Not patchable: the update path never emits it.
- Planned for insert: the insert input carries it as `Option<DateTime<Utc>>`
  where `None` means "use the timestamp the caller passed in" and `Some` copies
  an existing value, which is what importing old rows needs.

## `updated_at`

Marks the modification timestamp.

- Legal on: at most one field, optional.
- The field must be a `DateTime` and must not be `Option`.
- Not patchable: the update path always writes it from the `now` argument the
  caller passes in, ignoring any value in the row.
- Planned for insert: same `Option` semantics as `created_at`, defaulting to
  the creation timestamp rather than to "now", so an imported row does not look
  freshly modified.

## Legality rules enforced by the derive

Every rule below produces a compile error.

- The target must be a struct, not an enum or a union.
- The struct must have named fields.
- The struct must carry `#[db(table = "...")]` or `#[db(fragment)]`, exactly
  one of the two.
- `table` must be a non-empty string literal, and `table` and `fragment` cannot
  both be present.
- A table must not have generic parameters.
- A table row must declare `primary_key` at least once; a fragment must not
  carry it or any other role.
- Every field that is not `#[db(flatten)]` must carry `#[db(pgtype = "...")]`.
- `#[db(flatten)]` cannot be combined with `pgtype`, `pgdefault`, `query_ops`
  or a role, and its field must not be `Option`.
- A fragment cannot `#[db(flatten)]` another struct.
- `version`, `created_at` and `updated_at` may each appear at most once.
- A single field may carry at most one role, even if the roles are split across
  several `#[db(...)]` attributes; a primary key column therefore never also
  takes `version`, `created_at` or `updated_at`.
- A role must not be applied to an `Option` field.
- `version` must be `i64`.
- `created_at` and `updated_at` must be `DateTime`.
- An empty `#[db]` attribute is an error.
- `pgtype` must be a non-empty string literal and appear only once per field.
  It is not validated at compile time; the schema test runs it through
  PostgreSQL.
- `pgdefault` must be a non-empty string literal and appear at most once per
  field. It is not validated at compile time either.
- `query_ops` appears at most once per field, holds at least one operator, and
  every operator must be in the accepted list; repeating an operator is an
  error.
- The struct-level attribute accepts only `table` and `fragment`; the
  field-level attribute accepts only `pgtype`, `pgdefault`, `query_ops`,
  `flatten` and the four roles.

## How field types are matched

The derive compares the last path segment of the field type, so qualified
spellings behave the same:

- `DateTime<Utc>`, `chrono::DateTime<Utc>` and `sqlx::types::chrono::DateTime<Utc>`
  all count as `DateTime`.
- `Option<String>` and `std::option::Option<String>` both count as `Option`.
- Type aliases are not resolved. `type Timestamp = DateTime<Utc>;` on an audit
  column fails the `DateTime` check even though it is the same type.

Annotations do not affect `sqlx::FromRow`, serde or the database mapping. A
model still decodes every returned column, and the annotations only describe
the role a field plays for the generated code. In particular, `#[db(flatten)]`
splices columns into dbkit's metadata only: a `FromRow` model whose struct
should decode flat columns needs `#[sqlx(flatten)]` on the same field, and its
JSON stays nested unless the field also carries `#[serde(flatten)]`.

## Error messages

For reference when reading a compile failure:

- `` `DbModel` can only be derived for structs ``
- `` `DbModel` requires named fields ``
- `` missing `#[db(table = "...")]` or `#[db(fragment)]` ``
- ``unknown attribute; the struct accepts `table` or `fragment` ``
- `` `table` must not be empty ``
- `` duplicate `table` ``
- `` duplicate `#[db(fragment)]` ``
- `` `table` and `fragment` are mutually exclusive ``
- `` `DbModel` on a table cannot have generic parameters; ... ``
- ``unknown attribute; expected `primary_key`, `version`, `created_at`, `updated_at`, `flatten`, `pgtype`, `pgdefault` or `query_ops` ``
- `` a fragment cannot carry `#[db(...)]` roles ``
- `` a fragment cannot `#[db(flatten)]` another struct ``
- `` `#[db(flatten)]` cannot be combined with `pgtype` `` (and the same for
  `pgdefault`, `query_ops` and a role)
- `` `#[db(flatten)]` field must not be `Option` ``
- ``missing `#[db(pgtype = "...")]` on field `from` ``
- `` `pgtype` must not be empty ``
- ``duplicate `pgtype` ``
- `` `pgdefault` must not be empty ``
- ``duplicate `pgdefault` ``
- ``empty `#[db]` attribute``
- ``a field can carry at most one `#[db(...)]` role``
- `` missing `#[db(primary_key)]` ``
- ``duplicate `#[db(...)]` role``
- `` `#[db(primary_key)]` field must not be `Option` `` (and the same for each
  other role)
- `` `#[db(version)]` field must be `i64` ``
- `` `#[db(created_at)]` field must be a `DateTime` ``
- ``unknown query operator; expected `eq`, `neq`, ...``
- ``duplicate query operator``

All problems found in one struct are reported together, so a single compile
fixes the whole annotation block.

One check is not a compile error. `#[db(flatten)]` on a table type compiles,
because a table and a fragment are both `dbkit::Model`; the generated `meta()`
matches `Meta` and panics on the table variant:

- `` `#[db(flatten)]` field `size` has type `<table>`, which is a table; flattened fields must be `#[db(fragment)]` structs ``

## Generated metadata

`#[derive(DbModel)]` emits `impl dbkit::Model for Email` with
`fn meta() -> dbkit::Meta`, whose `Meta::Table` variant carries a
`dbkit::TableMeta`:

- `table` — the value of `#[db(table = "...")]`.
- `primary_key`, `version`, `created_at`, `updated_at` — the annotated field
  names, or `None` for absent optional roles.
- `columns` — every column in declaration order as
  `ColumnMeta { name, nullable, pg_type, pg_default, query_ops }`, where
  `nullable` is true exactly when the field type is `Option<_>`, `pg_type` is
  the `pgtype` annotation, `pg_default` is the `pgdefault` annotation or
  `None`, and `query_ops` is the `query_ops` list in declaration order or an
  empty slice.

A `#[db(fragment)]` struct emits `Meta::Fragment(&TableFragment { columns })`
instead, where `columns` is the same `ColumnMeta` list and there are no table,
key or role fields to fill in.

`meta()` is a function rather than a const for one reason: a `const` array
cannot splice another type's columns (`&[ColumnMeta { .. }, Size::meta().columns()]`
does not compile, because a `const` item cannot spread a slice). The generated
body builds a `Vec<ColumnMeta>`, pushes each plain column and extends the list
from each flattened field's `Meta::Fragment` columns, then leaks the slice into
a `static OnceLock<TableMeta>`. The first call does the allocation; every later
call returns the same `Meta::Table`, pointing at the cached `TableMeta`.

A `#[db(fragment)]` struct needs no splicing, so its `meta()` returns a
promoted `static Meta = Meta::Fragment(&TableFragment { columns: &[..] })`; the
whole value is a constant with no `OnceLock`. `Meta` is an enum, so a
fragment's value has no table name or key to lie about. Both shapes implement
`Model`, so the type system still cannot tell them apart at the trait level;
`flatten` matches the `Meta` variant at runtime, and `Meta::into_table()`
panics if a fragment is listed in `MODELS`.

The emitted code names `::dbkit::...`, so the deriving crate depends on
`dbkit` (which re-exports the derive).

## Generated patch

`#[derive(DbPatch)]` on a model emits a `<Model>Patch` struct next to it:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, DbModel, DbPatch)]
#[db(table = "user")]
pub struct User {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: UserId,
    #[db(pgtype = "text")]
    pub email: Email,
    #[db(pgtype = "timestamptz")]
    pub email_verified_at: Option<DateTime<Utc>>,
    #[db(version)]
    #[db(pgtype = "bigint")]
    pub version: i64,
}

// generated
#[derive(Debug, Clone, Serialize, Deserialize, garde::Validate)]
pub struct UserPatch {
    #[garde(skip)]
    pub id: UserId,
    #[garde(skip)]
    pub version: i64,
    #[serde(default, skip_serializing_if = "Patch::is_absent")]
    #[garde(dive)]
    pub email: Patch<Email>,
    #[serde(default, skip_serializing_if = "Patch::is_absent")]
    #[garde(skip)]
    pub email_verified_at: Patch<DateTime<Utc>>,
}
```

- The primary key is a required field, spelled exactly as in the model; a
  composite key contributes one required field per key column. A missing key is
  a deserialization error. Key columns identify the row and are therefore never
  patchable themselves.
- When the model has a `#[db(version)]` field, the patch carries it as a
  required field too, so an update states the version it expects. A model
  without one, such as an append-only table, gets a patch without a version.
- Every other column becomes a `dbkit::Patch<T>` field: a missing key is
  `Patch::Absent` (leave the column alone), an explicit `null` is `Patch::Null`
  (clear a nullable column) and any other value is `Patch::Set`.
- A nullable column's field is `Patch<T>`, not `Patch<Option<T>>`, so `null`
  means "clear" rather than "set to null".
- `created_at` and `updated_at` are not patchable and are omitted.
- A `#[db(flatten)]` field becomes a `#[serde(flatten)]` field of type
  `<FieldType>Patch`, so the fragment's columns are top-level keys of the same
  patch object. The fragment must therefore derive `DbPatch` too. On a
  `#[db(fragment)]` struct the generated patch has no required key or version
  field.
- The struct derives `Debug`, `Clone`, `Serialize` and `Deserialize`; patch
  fields carry `skip_serializing_if = "Patch::is_absent"`, so an absent field
  stays off the wire in a response.
- The struct derives `garde::Validate`. A field whose payload type can carry
  rules, such as a newtype like `Email`, is marked `#[garde(dive)]`, so a
  `Patch::Set` value is validated and `Patch::Absent` or `Patch::Null` passes.
  Primitive payloads and identifiers are `#[garde(skip)]`, because only a local
  type can implement `garde::Validate`.
- `dbkit` implements `garde::Validate for Patch<T>` for every `T: Validate`, so
  one attribute covers all three states.
- `garde` is not tied to a web framework: a handler deserializes the patch and
  calls `patch.validate()`, mapping an error to `422 Unprocessable Entity`.

## Collecting models

A derive sees one struct at a time and cannot discover its siblings, so the
crate lists its models explicitly once:

```rust
use dbkit::Model;
use dbkit::TableMeta;
use std::sync::LazyLock;

pub static MODELS: LazyLock<Vec<TableMeta>> =
    LazyLock::new(|| vec![email::Email::meta().into_table()]);
```

`Meta::into_table()` unwraps the `Meta::Table` variant into an owned
`TableMeta` (`TableMeta` is `Copy`), and panics for a fragment. A `LazyLock`
keeps the list a single shared value instead of rebuilding it per call.
`&MODELS` coerces to `&[TableMeta]`, which is what the schema check and the
tests take. Tests iterate `MODELS.iter()`; omitting a model from the list is
caught by the schema test, which fails on any table without a model. Fragments
are not tables, so they are not listed — and listing one panics instead of
silently checking a table named `""`. A schema test that covers a model with
`#[db(flatten)]` covers its fragment's columns through the model's
`meta().columns()`.

## Relation to the schema test

The per-service `tests/schema.rs` applies `schema.sql` and asks
`dbkit::schema::check_live_schema_against_models` whether `MODELS` agrees with
the database:

```rust
let problems = dbkit::schema::check_live_schema_against_models(&mut connection, MODELS)
    .await
    .unwrap();

assert!(
    problems.is_empty(),
    "model/schema mismatch:\n{}",
    problems.join("\n")
);
```

Each problem is one line naming the table and column, so a failing test says
what to fix. The check covers names and nullability from
`information_schema.columns`, tables without a model, types and defaults, and
a column declared more than once — which is what two `#[db(flatten)]` fragments
sharing a column name produce.
Types are compared structurally: the helper creates a temporary table whose
columns use the `pgtype` annotations and compares each column's `atttypid` and
`atttypmod` with the real table's, so length and precision must match and any
spelling PostgreSQL accepts works. Defaults are compared from the same probe
table: the helper adds each `pgdefault` as a `DEFAULT` clause and compares the
normalized expression PostgreSQL reports for it with the real column's default,
so `now()` and `current_timestamp` are equivalent and a missing or extra
default is reported.

The helper takes a single connection, not a pool, because its temporary table is
session-local:

```rust
let db = PgtestServer::start().await.database().await;
sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();
let mut connection = db.acquire().await.unwrap();
```

A model and `schema.sql` drifting apart is a test failure, not a runtime
surprise.

## Not yet supported

These are deliberately out of scope until a model needs them:

- Soft-delete columns (`deleted_at`) and any read filter convention around
  them.
- Column names that differ from field names.
- Non-`i64` version types.
- Building SQL from `query_ops`: the annotation only records which operators a
  column accepts.
- Roles inside a fragment, such as an embedded `Audit` fragment carrying
  `created_at`: the role would have to become the parent model's role, and
  uniqueness would then be a cross-type check.
- Nested fragments: a fragment cannot `#[db(flatten)]` another fragment. The
  restriction is the macro's, not the type's: a fragment's `meta()` could
  splice a sibling's columns at runtime, but fragments are meant to stay leaf
  groups.
