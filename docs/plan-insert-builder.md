# Insert builder

Status: implemented; see [Insert builder](insert-builder.md).

## Goal

`#[derive(DbInsert)]` gives a model an insert input type and a batch query, the
same way `DbPatch` gives it a patch input and `PatchQuery`:

```rust
let user = UserInsert::new(Email("new@example.com".to_owned()))
    .set_email_verified_at(Utc::now());

let inserted: Vec<User> = User::insert([user]).execute(&pool).await?;
```

The plan covers:

- the generated `{Model}Insert` type, its required fields and its builders;
- `{Entity}::insert(entries)` and the `InsertQuery` it returns;
- the SQL shape, transaction behavior and returned rows;
- the derive rules, tests and documentation that follow.

## Input type

### Required and optional columns

The insert input is built from the resolved column metadata, not from column
roles: a primary key, version or audit column is treated like any other column
when deciding whether it must be supplied.

- A column is **required** when its resolved default is `None` and it is not
  nullable. Its field is plain `T`.
- Every other column is **optional**. Its field is `Option<T>`. `None` omits the
  column from the `INSERT`, so PostgreSQL applies the column default, or stores
  `NULL` when the column has no default. `Some(value)` binds the value.
- A nullable column is optional even without a default: omitting it inserts
  `NULL`.
- A defaulted column is optional even when it is not nullable: omitting it lets
  the database fill it in.
- A composite key is handled one column at a time: a key column with a default
  (`uuidv7()`) is optional, one without a default is required.

### `pgdefault` forms

Omitting `pgdefault` means **no default**. Inheritance is explicit:

```rust
#[db(pgdefault = inherit)]    // use the field type's PG_DEFAULT
#[db(pgdefault = "now()")]    // the column has this default

// omitted: the column has no default
```

- `inherit` maps to `PgDefault::Inherited`. `DbModel` reads the default from
  `<T as DbType>::PG_DEFAULT`; `DbInsert` makes the field optional. For a
  non-null field the derive also emits a const assertion that the type has a
  default. A type without one is a compile error; remove `inherit` to declare
  no default, or give the type a default. A nullable field needs no assertion:
  omission is valid either way and stores the type default or `NULL`.
- A string literal maps to `PgDefault::Declared`. The field is optional.
- Omitting `pgdefault` maps to `PgDefault::None`, regardless of whether the
  field declares `pgtype`. A non-null field is required; a nullable field is
  optional and omission stores `NULL`.
- `pgdefault` is independent of `pgtype`: `pgtype` decides the column type, and
  `pgdefault` decides the default. Omitting `pgtype` still inherits `PG_TYPE`
  from the field type.
- `inherit` is a field-only annotation. A `DbType` struct still declares its
  default with a string literal or omits it.
- The parser accepts a string literal or the identifier `inherit` after `=`;
  any other identifier, including `none`, is a compile error. `inherit` is not
  a Rust keyword, so it needs no quoting.
- Bare `#[db(pgdefault)]` is removed. The field simply omits `pgdefault` when
  the column has no default.

The resulting insert shape is deterministic:

- `Declared` -> optional `Option<T>`.
- `Inherited` + non-null -> optional `Option<T>` plus the const assertion.
- `Inherited` + nullable -> optional `Option<T>`.
- `None` + non-null -> required `T`.
- `None` + nullable -> optional `Option<T>`.

For `Inherited`, the macro knows the category but not the value of
`DbType::PG_DEFAULT`. It generates the optional shape and the const assertion;
the compiler resolves the associated const and proves that the type really has
a default. If the proof fails, the field must remove `inherit` or the type must
gain a default. This is why the generated field type can stay concrete instead
of becoming a trait projection.

If `WidgetId` has no `pgdefault`, `#[db(pgdefault = inherit)]` on `Widget.id`
therefore fails at the assertion. Without the annotation, the field has no
default and `DbInsert` requires it. Deriving only `DbModel` still compiles and
records the selected default, which the schema test compares with the real
column. `DbPatch` is unaffected and still requires the primary key.

Omitting `pgtype` does not leave the column without a type: the field type must
implement `DbType`, and `DbModel` reads `PG_TYPE` and `PG_DEFAULT` from it. The
field simply does not repeat the type on the model.

For a typical table the id type carries the column type and default, and the
field declares the role and opts into the type default:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, DbType)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[sqlx(transparent)]
pub struct WidgetId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, DbModel, DbInsert)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: WidgetId,

    #[db(pgtype = "text")]
    pub name: String,

    #[db(pgtype = "text")]
    pub note: Option<String>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}
```

generates:

```rust
pub struct WidgetInsert {
    pub id: Option<WidgetId>,
    pub name: String,
    pub note: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub version: Option<i64>,
}
```

- `id` inherits both its column type and its default from `WidgetId`; the model
  field declares `primary_key` and opts in with `pgdefault = inherit`. That
  inherited default is why the insert field is `Option<WidgetId>`, and the
  generated const assertion verifies that `WidgetId::PG_DEFAULT` is `Some`.
- `name` is the only required field: it is non-null and has no default.
- `id`, `created_at`, `updated_at` and `version` are defaulted, so they are
  optional and normally omitted.
- `note` is nullable, so `None` stores `NULL`.
- A caller can still supply any optional value, which is what importing rows
  with known ids, timestamps or versions needs.

The existing `AGENTS.md` guideline changes with this work: insert inputs are
plain `T` for required columns and `Option<T>` for columns the database may
fill. A missing optional column is omitted from the `INSERT`; the repository
does not compute `uuidv7()`, `now()` or `0` itself.

### Serialization and validation

- The struct derives `Serialize`, `Deserialize` and `garde::Validate`.
- Optional fields carry `#[serde(default, skip_serializing_if = "Option::is_none")]`,
  so a missing key is `None` and never reaches the wire as `null`.
- A required field has no serde default: a missing key is a deserialization
  error.
- A field whose payload type carries `garde` rules derives `#[garde(dive)]`;
  a primitive or foreign type is skipped, mirroring `DbPatch`.
- The insert input is a request input: a handler deserializes it, calls
  `validate()`, then passes it to the builder.

### Constructor and setters

```rust
impl WidgetInsert {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn set_name(mut self, value: impl Into<String>) -> Self;
    pub fn set_id(mut self, value: impl Into<WidgetId>) -> Self;
    pub fn set_note(mut self, value: impl Into<String>) -> Self;
    pub fn set_created_at(mut self, value: DateTime<Utc>) -> Self;
    pub fn set_updated_at(mut self, value: DateTime<Utc>) -> Self;
    pub fn set_version(mut self, value: i64) -> Self;
}
```

- `new` takes the required fields in declaration order. A model with no required
  fields gets `new()` with no arguments.
- Required scalar fields are taken with `impl Into<T>`, so `String` accepts
  `"text"` while newtypes still take their own type.
- Optional fields start as `None`.
- Every scalar column gets a `set_<field>` method. For an optional column the
  setter stores `Some(value)`; for a required column it replaces the value.
- There is no `clear_<field>` method. On insert, "not given" means the database
  chooses, while "clear" would mean SQL `NULL`; the two do not share a builder
  name. A nullable column without a default is cleared by leaving it `None`.

### Flattened fragments

- `DbInsert` is also derived for a `#[db(fragment)]` type, producing
  `{Fragment}Insert`. A fragment insert cannot be applied on its own, but it is
  a valid building block.
- The parent field is `#[serde(flatten)] pub size: SizeInsert`, matching the
  patch's nested shape.
- The parent constructor takes each flattened insert as an argument, because a
  fragment may itself contain required columns. A fragment without required
  columns is passed as `SizeInsert::new()`.
- A flattened field also gets `set_<field>({Fragment}Insert)` for replacing the
  whole fragment insert.
- A flattened column keeps its declaration position in the rendered `INSERT`.

Example:

```rust
let size = SizeInsert::new(640).set_note("wide");
let frame = FrameInsert::new(size, "photo");
```

### Defaults and audit columns

- The builder does not run a default expression in Rust. It omits the column and
  lets PostgreSQL evaluate `pgdefault`. This keeps the insert aligned with the
  schema test and avoids duplicating `uuidv7()`, `now()` or `0` in client code.
- A defaulted `version` is optional and normally omitted, so the database stores
  its default. Supplying it is an import path.
- Defaulted `created_at` and `updated_at` are optional and normally omitted, so
  both get the transaction timestamp.
- For an import where `created_at` is supplied but `updated_at` is not, the
  renderer also writes `updated_at` with the supplied creation time. An imported
  row then does not look freshly modified. The rule applies only when the model
  has both columns and `updated_at` was not explicitly supplied.
- A model without `created_at`, `updated_at` or `version` simply has fewer
  optional fields.

A nullable column that also has a default is the one case `Option<T>` cannot
express completely: `None` applies the default, so the caller cannot ask for an
explicit SQL `NULL`. No current model has such a column. If one appears, the
field can become a three-state insert value (`Default`, `Value`, `Null`) without
changing the rest of the builder.

## Query layer

### Generated method

For a table, the derive adds the sibling of `::query()` and `::patch()`:

```rust
impl Widget {
    pub fn insert(
        entries: impl IntoIterator<Item = WidgetInsert>,
    ) -> dbkit::InsertQuery<WidgetInsert> {
        dbkit::InsertQuery::new(entries)
    }
}
```

- One row is `Widget::insert([widget])`; several are
  `Widget::insert([first, second])` or a `Vec`.
- Rust has no variadic methods, so "multiple" means any `IntoIterator`, usually
  an array or a `Vec`.
- A fragment or subquery does not get `::insert`.
- A single-row convenience `Widget::insert_one(widget)` can be added if the
  array spelling proves noisy. It would return the same `InsertQuery` with one
  entry, so behavior stays identical.

### `InsertQuery`

```rust
pub struct InsertQuery<P: InsertRow> {
    entries: Vec<P>,
}

impl<P: InsertRow> InsertQuery<P> {
    pub fn new(entries: impl IntoIterator<Item = P>) -> Self;

    pub fn render(&self) -> Result<Vec<RenderedInsert>, InsertError>;

    pub async fn execute<'c, A>(
        self,
        acquirer: A,
    ) -> Result<Vec<P::Model>, InsertError>
    where
        A: Acquire<'c, Database = Postgres>,
        P::Model: for<'r> FromRow<'r, PgRow> + Send + Unpin;
}
```

- `render` returns one `RenderedInsert` per entry, for assertions and logs.
- `execute` renders every entry before opening a transaction, then runs the
  statements and collects the inserted rows.
- `RenderedInsert` mirrors `RenderedPatch`:

  ```rust
  pub struct RenderedInsert {
      pub sql: String,
      pub arguments: PgArguments,
      pub values: Vec<serde_json::Value>,
  }
  ```

- `InsertRow` is the generated bridge:

  ```rust
  pub trait InsertRow {
      type Model: Model;

      /// Provided columns in declaration order. An absent optional column is
      /// not in the list; a present column always has a value.
      fn columns(&self) -> Vec<Assignment<'_>>;
  }
  ```

  `Assignment` is the existing `{ column, value }` pair from `dbkit::patch`.
  Its docs are generalized from "one column of a patch" to "one written column"
  so both builders can use it.

  The expansion distinguishes required from optional fields:

  ```rust
  impl InsertRow for WidgetInsert {
      type Model = Widget;

      fn columns(&self) -> Vec<Assignment<'_>> {
          let mut columns = Vec::new();

          if let Some(id) = &self.id {
              columns.push(Assignment { column: "id", value: Some(id) });
          }
          columns.push(Assignment { column: "name", value: Some(&self.name) });
          if let Some(note) = &self.note {
              columns.push(Assignment { column: "note", value: Some(note) });
          }
          // created_at, updated_at and version follow the same optional form.

          columns
      }
  }
  ```

### SQL shape

A single entry renders one `INSERT` with `RETURNING *`:

```sql
INSERT INTO "widget" ("name")
VALUES ($1)
RETURNING *
-- params: ["Thingie"]
```

Two provided optional columns add placeholders:

```sql
INSERT INTO "widget" ("name", "note")
VALUES ($1, $2)
RETURNING *
```

An entry with every column optional and every field `None` renders:

```sql
INSERT INTO "widget"
DEFAULT VALUES
RETURNING *
```

Supplying import values renders them explicitly:

```sql
INSERT INTO "widget" ("name", "created_at", "updated_at", "version")
VALUES ($1, $2, $3, $4)
RETURNING *
```

- The renderer walks `TableMeta::columns` and emits the provided assignments in
  that order, so flattened columns and a synthesized `updated_at` sit in their
  declaration positions.
- Only provided columns appear. Omitted defaulted columns are left to the
  database.
- `RETURNING *` decodes into the model, so the caller sees the generated id,
  timestamps and version, not just the values it supplied.
- The table and column names come from `TableMeta`, never from a request.

### One statement per entry

The first version renders one `INSERT` per entry and runs them in one
transaction.

- Entries can provide different optional columns, so a single multi-row
  `VALUES` list would need one uniform column list; filling differences with
  `NULL` would defeat the column defaults.
- Separate statements preserve the input order of the returned rows.
- The transaction makes the batch all-or-nothing.
- A future optimization can group entries with identical provided columns into
  one multi-row `INSERT ... VALUES (...), (...)`; it is not part of this plan.

### Transactions and nesting

- `execute` takes `sqlx::Acquire`, exactly like `PatchQuery::execute`.
- A pool or a plain connection opens a transaction and commits it only after
  every entry succeeded.
- A `&mut Transaction` creates a savepoint, so the caller can compose inserts
  with other work in one outer transaction. A failed nested batch restores its
  savepoint and leaves the outer transaction usable.
- An empty entry list is a no-op and returns an empty `Vec` without opening a
  transaction.

```rust
let mut transaction = pool.begin().await?;

User::insert([first]).execute(&mut *transaction).await?;
User::insert([second]).execute(&mut *transaction).await?;

transaction.commit().await?;
```

### Errors

```rust
pub enum InsertError {
    /// The model is a fragment or a subquery, not a table.
    NotATable,
    /// An `INSERT ... RETURNING *` did not return exactly one row.
    NotInserted { index: usize },
    /// Encoding, execution, decoding, savepoint or commit failure.
    Sqlx(sqlx::Error),
}
```

- `NotATable` is the same guard as `PatchError::NotATable`. The generated table
  method cannot reach it.
- `NotInserted` is a guard around `RETURNING *`; a successful insert of one row
  returns one row.
- Unique, foreign-key, check and not-null violations stay wrapped in
  `InsertError::Sqlx`. A handler that maps a duplicate key to `409 Conflict`
  inspects the database constraint through `sqlx::Error::Database`.
- There is no `Empty` variant: `DEFAULT VALUES` is a valid insert, and an empty
  batch is a no-op.

### Out of scope

- No `ON CONFLICT` or upsert yet. A later `on_conflict` builder can sit on
  `InsertQuery` without changing the generated input.
- No `RETURNING` column selection: the first version always returns the whole
  model.
- No multi-row statement optimization in the first version.
- No client-side default evaluation.

## Derive rules

- `DbInsert` reads the same `#[db(...)]` annotations as `DbModel` and
  `DbPatch`.
- A `#[db(subquery = "...")]` model is rejected: it is read-only.
- Tables and fragments are accepted. The fragment form exists for
  `#[db(flatten)]`.
- Requiredness follows the resolved `PgDefault`: `Declared` and `Inherited` are
  optional, while `None` is required when non-null and optional when nullable.
  Omitting `pgdefault` selects `None`, `inherit` selects `Inherited`, and a
  string selects `Declared`. `inherit` on a non-null field emits a const
  assertion that the type has a default.
- The patch struct visibility rule carries over: the insert struct follows the
  model's visibility, so a private model gets a private insert type.
- The generated struct is `Debug + Clone + Serialize + Deserialize + Validate`.
- The generated impl is `dbkit::InsertRow`.
- The generated method is `{Entity}::insert` for tables only.
- `DbInsert` may share `field_text`, `non_optional`, `carries_rules` and the
  patch-type naming helper with `derive_patch`; the two derives stay separate
  where their field rules differ.

## Type-level resolution through `DbType`

The macro cannot read `DbType::PG_DEFAULT`, but the compiler can. The “normal
Rust trait resolution” path is to encode the default as a type, not only as a
const:

```rust
pub trait DbType {
    const PG_TYPE: &'static str;
    const PG_DEFAULT: Option<&'static str> = None;

    /// `Self` when the column has no default, `Option<Self>` when it does.
    type InsertField;
}
```

`#[derive(DbType)]` already sees the type-level `pgdefault` annotation, so it
can set the associated type:

- a type with `#[db(pgdefault = "...")]` gets `type InsertField = Option<Self>`;
- a type without a default gets `type InsertField = Self`.

A separate `InsertShape: DbType` trait with the same associated type would keep
`DbType` itself unchanged; the `DbType` derive can implement both, and
`DbInsert` projects through `InsertShape` only for inherited fields. Manual
`DbType` implementations then need the extra trait only when a model uses them
on insert.

`DbInsert` then emits an inherited non-null field as:

```rust
pub id: <WidgetId as DbType>::InsertField,
```

The compiler resolves the projection, so the macro never needs the const value.
This is ordinary trait resolution, not macro state sharing. It works across
modules and crates, under incremental builds, and without expansion-order
assumptions.

### What it buys

- An inherited type with a default becomes optional automatically.
- An inherited type without a default becomes required automatically.
- Explicit `pgtype`/`pgdefault` fields keep their current annotation-based
  classification; only the inherited case changes.
- A nullable field is already `Option<T>` from the model field syntax and does
  not use the projection, because `T::InsertField` may already be `Option<T>`
  and a double option is not wanted.

### What it costs

- The generated field type is a projection rather than a plain `T` or
  `Option<T>`. Rustdoc, compiler errors and the public API show the projection
  instead of the concrete shape.
- Serde is the main problem. A required `T` field must not have
  `#[serde(default)]`, while an optional `Option<T>` field must. The derive no
  longer knows which one it is for an inherited field, so it cannot simply
  derive `Deserialize`. It would need a custom deserializer, a wrapper field
  type, or a helper trait that distinguishes “missing is an error” from
  “missing is `None`”.
- `garde` has the same conditional problem for any future `required`/`default`
  rule. `#[garde(dive)]` itself works for both `T` and `Option<T>`.
- The constructor and setters need the same distinction: `new` takes a required
  value, while an optional field starts `None`. A helper trait can construct and
  inspect both shapes, but the generated API becomes more indirect.
- Every manual `DbType` implementation must provide the new associated type if
  it is added to `DbType`; with a separate `InsertShape` trait, only types used
  on insert need it.
- A field inherits the type default only when it says `pgdefault = inherit`;
  `pgtype` does not change that. The projection applies only to the
  explicit-inherit case, so the derive still needs both code paths.

### Would it help patches?

No. `DbPatch` already has everything it needs at macro expansion time, and
database defaults are deliberately irrelevant to it:

- the primary key and version are roles, so the derive knows they are required;
- every business column is `Patch<T>` regardless of whether the column has a
  default;
- `Patch::Absent` means “leave the column unchanged”, not “use the database
  default”, so an insert-field projection would be semantically wrong;
- a primary key with a default is still required in a patch, because an update
  must name its row;
- a version with a default is still required in a patch, because the update
  needs the current optimistic-locking token.

Making patch fields depend on `T::InsertField` would conflate “this column has
a database default” with “the caller may omit this patch field”. The latter is
already `Patch::Absent` for every business column, independent of any default.
The shared piece between the derives is the `DbType` metadata itself, not the
insert shape.

`DbModel` and `DbSelect` do not need the projection either: `DbModel` already
resolves `PG_TYPE` and `PG_DEFAULT` at runtime, and `DbSelect` takes its shape
from the model field types. `DbInsert` is the only derive whose generated
struct shape depends on whether a column has a default.

### A cheaper use of trait resolution

The compiler can check the `inherit` case without changing field types:

```rust
const _: () = assert!(
    <WidgetId as DbType>::PG_DEFAULT.is_some(),
    "field `id` says `pgdefault = inherit`, but `WidgetId` has no default; \
     remove the annotation or give the type a default",
);
```

- This uses trait resolution for a compile-time guard, not for field selection.
- Concrete `T`/`Option<T>` fields, `serde`, `garde` and the generated setters
  stay as they are.
- `inherit` on a type without a default becomes a compile error with an
  actionable message, instead of an optional field that fails at runtime with
  `NOT NULL`.
- It has no effect on patches.

The trade-off is that the fix is an annotation: the model removes `inherit` to
declare no default, or gives the type a default.

### Recommendation

Start with the annotation-based rule plus the const assertion. It keeps the
generated API concrete and the derives simple. Adopt the associated-type
projection only if exact automatic classification for inherited fields is worth
the custom deserialization, conditional constructors and less direct public
types. Do not use macro state sharing or a `#[dbmarker]` side channel: proc
macros cannot provide it reliably.

## Tests

### Rendering tests

Add `crates/dbkit/tests/insert_query.rs` in the style of `patch_query.rs`:

- a required-only insert renders one column and one placeholder;
- an optional field left `None` is omitted;
- an optional field set with `Some` adds a column and a placeholder;
- an all-optional entry renders `DEFAULT VALUES`;
- a model without version or audit columns renders without them;
- a composite key with one defaulted and one required key renders only the
  supplied key column;
- a flattened fragment contributes its columns in declaration order;
- a supplied `created_at` adds `updated_at` with the same value when
  `updated_at` is not supplied;
- an empty batch renders nothing;
- `new`, `set_*`, required-field defaults and flattened setters build the
  expected struct;
- missing required fields and `null` for optional fields deserialize as
  expected;
- validation dives into required payloads and ignores `None` optionals.

### Live tests

Add `crates/dbkit/tests/insert_live.rs` in the style of `patch_live.rs`:

- inserting one row omits defaults and returns the database-generated id,
  timestamps and version;
- inserting several rows returns them in input order;
- explicit id, timestamps and version are preserved;
- a supplied `created_at` also sets `updated_at`;
- an omitted nullable column is `NULL`;
- a unique violation rolls back every earlier entry in the batch;
- a nested batch uses a savepoint and leaves the outer transaction usable;
- an empty batch is a no-op;
- a composite key inserts and returns its row.

### Derive tests

- `DbInsert` rejects a subquery.
- A defaulted primary key is optional; a primary key without a default is
  required.
- A nullable column is optional even without a default.
- The generated visibility matches the model.
- A fragment derives an insert type but no `::insert` method.
- The annotation parser accepts `pgdefault = "..."` and `pgdefault = inherit`;
  bare `pgdefault`, `pgdefault = none` and unknown identifiers are errors.
- Omitting `pgdefault` means no default, whether or not `pgtype` is declared.
- `pgdefault = inherit` uses the type's `PG_DEFAULT`; without `inherit`, the
  field has no default whether or not `pgtype` is declared.
- `inherit` makes a non-null insert field optional and emits the const
  assertion; a type without a default fails to compile. A nullable inherited
  field stays optional without the assertion.
- Omitting `pgdefault` makes a non-null field required and a nullable field
  optional.
- Repeating `pgdefault` is a duplicate error.

Live tests need the Podman-backed `pgtest-server`; the current environment
cannot start it, so they are written and compiled here and run where rootless
containers work.

## Documentation

- Added `docs/insert-builder.md` as the user-facing reference, mirroring
  `docs/patch-builder.md`.
- Updated the "Generated insert" material in `docs/dbkit-annotations.md`.
- Updated the `pgdefault` section and the annotation legality rules in
  `docs/dbkit-annotations.md`: omission means no default, `inherit` opts into
  the type default, and bare `#[db(pgdefault)]` is removed. `pgtype` no longer
  affects whether a default is inherited.
- Migrated existing models: `User.id`, `UserSession.id` and `Email.id` add
  `#[db(pgdefault = inherit)]`; `UserSession.user_id` drops its bare
  `#[db(pgdefault)]`.
- Updated the `AGENTS.md` insert-input guideline to the required/optional split.
- Updated the crate docs in `crates/dbkit/src/lib.rs` and the `DbInsert` macro
  docs in `crates/dbkit-macros/src/lib.rs`.

## Implementation order

1. Change the `pgdefault` parser and `PgDefault` mapping so omission means
   `None`, `inherit` means `Inherited`, and a string means `Declared`; remove
   bare `pgdefault` and reject `none`.
2. Add parser tests for both forms, omission, duplicates and the
   explicit-`pgtype` cases; migrate the defaulted id fields and
   `UserSession.user_id`, and update tests that still use bare `pgdefault`.
3. Add `dbkit/src/insert/` with `InsertRow`, `InsertQuery`, `RenderedInsert` and
   `InsertError`; export them from the crate root.
4. Generalize the `Assignment` docs and reuse the identifier helper.
5. Add `derive_insert.rs`, register `DbInsert`, and generate the struct,
   constructor, setters, const assertions, `InsertRow` impl and
   `Entity::insert`.
6. Add rendering and builder tests.
7. Add live tests.
8. Write the reference documentation and update the annotation/macro docs.
9. Run `cargo +nightly fmt --all`, `cargo clippy --all-targets -- -D warnings`
   and `cargo nextest run`.

## Decisions

- Required columns are plain `T` and optional columns are `Option<T>` (rather
  than wrapping every column in a three-state value). This is the simplest shape
  that enforces the common case at compile time; the nullable-plus-default case
  is documented as unsupported for now.
- `execute` returns the inserted rows, because the database fills in the
  defaults. A caller that only wants success can ignore the `Vec`.
- Entries run as one statement per entry in one transaction. This keeps default
  semantics and input order simple; grouping compatible entries into a
  multi-row `INSERT` can be added later.
- A supplied `created_at` also supplies `updated_at` when the latter is omitted,
  so imported rows keep their historical modification time.
- `pgdefault` accepts `inherit` or a string; omitting it means no default. Bare
  `#[db(pgdefault)]` and `none` are removed as redundant spellings. This makes
  insert requiredness determinable from the field annotation without a
  type-level projection. `inherit` is verified by a const assertion, so a type
  without a default fails at compile time.
- Making omission mean `None` is a metadata change: a field that previously
  inherited its type default now has none unless it says
  `#[db(pgdefault = inherit)]`. The migration is limited to the defaulted id
  fields and the foreign-key field listed above.
- `DbPatch` ignores the new forms. A database default never makes a patch field
  optional, because `Patch::Absent` already means “leave the column unchanged”.
- The type-level projection in “Type-level resolution through `DbType`” remains
  the exact alternative, but its serde and constructor costs make it a follow-up
  rather than part of the first version.
