# dbkit annotations

Annotations understood by `#[derive(DbModel)]` from the `dbkit-macros` crate.
They describe the fixed columns of a table row: which fields form the primary
key, which one carries the optimistic-locking token, which ones are audit
columns, which columns the database fills in from a default, and which
comparisons each column may be queried with. Every field carries a `pgtype`
unless it is flattened or its type declares one with `#[derive(DbType)]`, and
a table row needs a primary key; the other annotations are opt-in.

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
The same `#[derive(DbModel)]` expansion also generates the model's selectable
column enum, `<Model>Column`. The accepted syntax stays as documented here.

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
- `#[db(pgtype = "...")]` on every field that is not `#[db(flatten)]`, unless
  the field type implements `dbkit::DbType`, exactly once.
- `#[db(pgdefault = "...")]` on a field, at most once.
- `#[db(pgtype = "...")]` and `#[db(pgdefault = "...")]` on the struct of a
  value type that derives `DbType`, instead of on a field.
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
- On a `DbType` struct, `#[db(pgtype = "...")]` sits above
  `#[db(pgdefault = "...")]`, after the `#[derive(...)]` line and before the
  serde and sqlx attributes.
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

## `DbType`

Moves the column type and its default to the value type, so a model field of
that type repeats neither.

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, DbType)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[sqlx(transparent)]
pub struct UserSessionId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, DbModel)]
#[db(table = "user_session")]
pub struct UserSession {
    #[db(primary_key)]
    pub id: UserSessionId,

    // `UserId` declares a default too, but this column has none.
    #[db(pgdefault)]
    pub user_id: UserId,

    // ...
}
```

- `#[db(pgtype = "...")]` on the struct is required and takes the same values
  as a field's `pgtype`; `#[db(pgdefault = "...")]` is optional.
- Generates `impl dbkit::DbType`, whose `PG_TYPE` and `PG_DEFAULT` constants the
  model derive reads. A field that omits `pgtype` takes both from the type;
  `Option<T>` inherits from `T` and stays nullable.
- A field that declares its own `pgtype` keeps only the `pgdefault` it declares
  itself. This is why plain types such as `DateTime<Utc>` and `String` need no
  `DbType` implementation: their fields spell out `pgtype` and declare no
  default.
- The type-level default is a suggestion: the same value type is used for
  foreign keys, whose columns have no default. A bare `#[db(pgdefault)]` on the
  field states that the column has none.
- Field annotations are not accepted on a `DbType` struct; a missing `pgtype`
  is an error.
- A field whose type does not implement `DbType` and has no `pgtype` fails to
  compile with a trait-bound error naming the type.

## `pgtype`

Names the column type, written the way it is written in `schema.sql`.

- Legal on: a field that is not `#[db(flatten)]`, or the struct of a `DbType`
  type. Required on a field unless the field type implements `dbkit::DbType`.
- Value: a non-empty string literal holding a SQL type: `uuid`, `text`,
  `bigint`, `boolean`, `timestamptz`, `jsonb`, `varchar(255)`,
  `numeric(10,2)`, `text[]`, `text[][]`.
- An empty or repeated `pgtype` is an error. A field that omits it takes one
  from its type (see [`DbType`](#dbtype)); a `DbType` struct that omits it is
  an error.
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

- Legal on: a field or a `DbType` struct, optional and at most once.
- Value: a non-empty string literal holding a default expression: `uuidv7()`,
  `now()`, `0`, `'{}'::jsonb`. A bare `#[db(pgdefault)]` on a field declares
  that the column has no default.
- Omitted when the column has no default. An empty or repeated `pgdefault` is
  an error. A field that declares no `pgtype` inherits the type's default
  unless it carries the bare form.
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
  `not_ilike`, `in`, `not_in`, `contains`, `not_contains`, `overlaps`,
  `not_overlaps`, `contains_all`, `not_contains_all`, `is_null`, `is_not_null`,
  `between`, `not_between`. They map one-to-one onto `dbkit::FilterOperator`,
  and each name is also the JSON key suffix and the method on the column
  handle.
- Order is preserved and duplicates are an error.
- The list decides what a column can be filtered with, on both faces of
  [`Query`](query-builder.md): the derive implements one capability trait per
  entry for the column's tag type, so an operator that is not listed does not
  compile, and the JSON deserializer rejects it at run time. The annotation
  still records the list in `Column::query_ops` and generates no SQL.
- Columns without the annotation get an empty list, meaning "not filterable"
  rather than "every operator allowed".
- `is_null` and `is_not_null` need no declaration: they take no value and cannot
  be mistyped, so every column has them.
- The list is not checked against the column type. `like` on a `uuid` column
  compiles and reaches the database, which rejects it; declaring an operator
  whose type the column does not have is a model mistake the schema test cannot
  see.

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
- Every field that is not `#[db(flatten)]` must carry `#[db(pgtype = "...")]`
  or have a type that implements `dbkit::DbType`.
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
- `pgtype` must be a non-empty string literal and appear only once per field or
  `DbType` struct. It is not validated at compile time; the schema test runs it
  through PostgreSQL.
- `pgdefault` must be a non-empty string literal when it has a value, and must
  appear at most once per field or `DbType` struct. A bare `#[db(pgdefault)]`
  is accepted on a field only and means "no default". It is not validated at
  compile time either.
- `query_ops` appears at most once per field, holds at least one operator, and
  every operator must be in the accepted list; repeating an operator is an
  error.
- The `DbModel` struct attribute accepts only `table` and `fragment`; the
  field-level attribute accepts only `pgtype`, `pgdefault`, `query_ops`,
  `flatten` and the four roles. The `DbType` struct attribute accepts only
  `pgtype` and `pgdefault`.
- A `DbType` struct must not annotate its fields, and it must carry `pgtype`.

## How field types are matched

The derive compares the last path segment of the field type, so qualified
spellings behave the same:

- `DateTime<Utc>`, `chrono::DateTime<Utc>` and `sqlx::types::chrono::DateTime<Utc>`
  all count as `DateTime`.
- `Option<String>` and `std::option::Option<String>` both count as `Option`.
- An `Option<T>` field takes `DbType` metadata from `T`, so the column keeps
  the type's `pg_type` and `pg_default` while `nullable` becomes true.
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
- `` `pgtype` must not be empty ``
- ``duplicate `pgtype` ``
- `` `pgdefault` must not be empty ``
- ``duplicate `pgdefault` ``
- `` `DbType` can only be derived for structs ``
- `` `DbType` takes its annotations on the struct, not on a field ``
- ``unknown attribute; the struct accepts `pgtype` or `pgdefault` ``
- `` missing `#[db(pgtype = "...")]` `` (on a `DbType` struct)
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

A field with neither `pgtype` nor a type that implements `DbType` is not caught
by the derive, because it cannot resolve types; rustc reports the unsatisfied
`dbkit::DbType` bound on the field type instead.

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
  `&'static dyn dbkit::ErasedColumn`, whose `name()`, `nullable()`,
  `pg_type()`, `pg_default()` and `query_ops()` answer like the generated
  `<Model>Column` variants. `nullable` is true exactly when the field type is
  `Option<_>`, `pg_type` is the field's `pgtype` annotation or its `DbType`
  type's `PG_TYPE`, `pg_default` is its `pgdefault` annotation, its `DbType`
  type's `PG_DEFAULT`, or `None`, and `query_ops` is the `query_ops` list in
  declaration order or an empty slice.

A `#[db(fragment)]` struct emits `Meta::Fragment(&TableFragment { columns })`
instead, where `columns` is the same erased list and there are no table, key or
role fields to fill in.

The list holds trait objects because a `#[db(flatten)]` field splices another
type's columns, so one table's description can mix columns from several models
and no single enum can name them all. The columns are the `<Model>Column`
variants the same expansion emits; the macro pushes each variant's reference
and extends the list from each flattened field's `Meta::Fragment` columns, then
leaks the slice into a `static OnceLock<TableMeta>`. The first call does the
allocation; every later call returns the same `Meta::Table`, pointing at the
cached `TableMeta`. A fragment uses the same shape with a
`OnceLock<TableFragment>`, so both shapes build their columns one way.

`Meta` is an enum, so a fragment's value has no table name or key to lie about.
`Meta::column(name)` looks a column up by name. Both shapes implement
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

## Generated columns

`#[derive(DbModel)]` on a model emits a `<Model>Column` enum naming every
column, in declaration order, plus the `dbkit::Column` implementation that
`dbkit::Columns` selects from:

```rust
#[derive(DbModel)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: WidgetId,
    #[db(pgtype = "text")]
    pub name: String,
    #[db(pgtype = "text")]
    pub email_verified_at: Option<String>,
    #[db(version)]
    #[db(pgtype = "bigint")]
    pub version: i64,
}

// generated
pub enum WidgetColumn {
    Id,
    Name,
    EmailVerifiedAt,
    Version,
}

impl WidgetColumn {
    pub fn all() -> &'static [Self] {
        &[Self::Id, Self::Name, Self::EmailVerifiedAt, Self::Version]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::EmailVerifiedAt => "email_verified_at",
            Self::Version => "version",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> { /* inverse of name() */ }
    pub fn pg_type(self) -> &'static str { /* "uuid", "text", ... */ }
    pub fn pg_default(self) -> Option<&'static str> { /* Some("uuidv7()"), ... */ }
    pub fn nullable(self) -> bool { /* ... */ }
    pub fn query_ops(self) -> &'static [FilterOperator] { /* ... */ }
}

impl Serialize for WidgetColumn { /* serialize_str(self.name()) */ }
impl<'de> Deserialize<'de> for WidgetColumn { /* from_name() */ }

// one empty tag per column, one capability per query_ops entry, and the typed
// handles the query builder takes
pub struct WidgetIdTag;
impl<C: Column> dbkit::HasEq<C> for WidgetIdTag {}

pub struct WidgetColumns<C: Column = WidgetColumn> {
    pub id: dbkit::ColumnRef<WidgetIdTag, C, WidgetId>,
    pub name: dbkit::ColumnRef<WidgetNameTag, C, String>,
    // ...
}

impl Widget {
    pub fn columns() -> WidgetColumns { /* ... */ }
    pub const fn query() -> dbkit::Query<WidgetColumn> { /* ... */ }
}
```

`WidgetColumns::map` rewraps every handle onto another column enum, which is how
a `#[db(flatten)]` parent turns a fragment's handles into its own. The tags and
the handle struct are public because they appear in the field types; nothing in
the API asks a caller to name them.

- A variant is the only proof that a column exists, and `name()` returns a
  `&'static str`, so a query layer builds SQL from the enum and never formats a
  request string into the statement. A name that is not a column fails to
  deserialize, and the SQL identifier cannot be bound as a parameter.
- Variants are PascalCase. Serde is written by hand: a column serializes as its
  database name, and an unknown name reports the column list, so the wire form
  is `["id", "email_verified_at"]` rather than the Rust name.
- Role columns are selectable: a patch exposes business columns only, but a read
  needs the key, the version and the audit columns.
- `dbkit::Columns<C>` is the selection set. It deserializes from an array of
  column names, iterates in the order the columns were added, and offers
  `all()`, `insert`, `remove`, `contains`, `names()`, `pg_types()` and `len()`.
  `all()` adds them in declaration order; a request keeps its own.
- `Column::all()` is a promoted slice when the model has no flattened field and
  a cached `Vec` when it does, because a nested enum's variants cannot be listed
  at expansion time.
- A `#[db(fragment)]` struct gets its own `<Fragment>Column`. A table with
  `#[db(flatten)]` gets a variant wrapping it:

  ```rust
  #[derive(DbModel)]
  #[db(table = "frame")]
  pub struct Frame {
      #[db(primary_key)]
      #[db(pgtype = "uuid")]
      pub id: Uuid,
      #[db(flatten)]
      pub size: Size,
      #[db(pgtype = "text")]
      pub label: String,
  }

  // generated: the fragment's columns are selected through the wrapper
  pub enum FrameColumn {
      Id,
      Size(SizeColumn),
      Label,
  }

  FrameColumn::Size(SizeColumn::Width).name(); // "width"
  ```

  `all()` places the wrapped columns at the field's position, so `FrameColumn`
  lists `id`, `width`, `height`, `label`. `from_name("width")` returns
  `Some(FrameColumn::Size(SizeColumn::Width))`, and a request picks a flattened
  column like any other.

### Column metadata

The enum is the only description of a column. It carries the value and default
inline, so these are plain lookups with no metadata search:

```rust
WidgetColumn::Id.pg_type();                // "uuid", from the field type's DbType
WidgetColumn::EmailVerifiedAt.nullable();  // true
WidgetColumn::Id.pg_default();             // Some("uuidv7()")
WidgetColumn::Name.query_ops();            // [Eq, Neq, Like], from #[db(query_ops(...))]
```

A model's `TableMeta::columns` holds the same values as
`&'static dyn ErasedColumn`; `dbkit` implements `ErasedColumn` for every
`Column`, so the erased view is a forward, not a second description. That is
what the JSON face of [`Query`](query-builder.md) reads to pick the concrete
type of a filter value, and what `build()` reads to reject an operator the
column does not declare.

A request type holds `Option<Columns<C>>`: absent means every column, and an
explicitly empty list is an error rather than a second spelling of "all".

`dbkit::Query` is what turns a [`Columns`](query-builder.md) selection and a
tree of filters into SQL, so a handler does not build the statement:

```rust
let query = Widget::query()
    .select(request.select.unwrap_or_else(Columns::all))
    .filters([Widget::columns().name.eq("example")]);
let built = query.build()?;
let rows = sqlx::query_as_with(AssertSqlSafe(built.sql), built.arguments)
    .fetch_all(&pool)
    .await?;
```

`sqlx::FromRow` cannot decode a partial projection: the derived implementation
reads every field by name. A sparse selection therefore only suits a response
built dynamically or a projection type that matches the requested columns.

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
- Roles inside a fragment, such as an embedded `Audit` fragment carrying
  `created_at`: the role would have to become the parent model's role, and
  uniqueness would then be a cross-type check.
- Nested fragments: a fragment cannot `#[db(flatten)]` another fragment. The
  restriction is the macro's, not the type's: a fragment's `meta()` could
  splice a sibling's columns at runtime, but fragments are meant to stay leaf
  groups.
- A fragment flattened twice in one table: both fields contribute a variant, and
  `from_name` returns the first match, so a duplicate column name resolves to
  one of them. The schema test reports the duplicate.
- Marking a column unselectable: every column becomes a `<Model>Column`
  variant today, so restricting a sensitive column such as a token hash needs a
  new annotation.
