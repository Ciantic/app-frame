# Patch builder

A `#[derive(DbPatch)]` model gets a sibling of `::query()`:

```rust
User::patch([
    user_patch_one,
    user_patch_two,
])
.execute(&pool)
.await?;
```

`::patch(entries)` builds a `PatchQuery<UserPatch>`, renders one `UPDATE` per
entry, and runs them in one transaction. Each entry names its row by primary
key, checks the version it expects, and changes only the columns it sets.

The input is the same [`Patch`](#the-patch-input) struct used by a single
update; the builder adds the batch and transaction behavior.

## The patch input

A model's patch separates the three states of a field: not given, set to a
value, and set to `NULL`.

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

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    pub version: i64,
}

// generated
pub struct UserPatch {
    pub id: UserId,
    pub version: i64,
    pub email: Patch<Email>,
    pub email_verified_at: Patch<DateTime<Utc>>,
}
```

- The primary key and version are required, plain fields.
- Every other business column is a `Patch<T>`:
  - `Patch::Absent` leaves the column alone;
  - `Patch::Set(value)` writes the value;
  - `Patch::Null` writes SQL `NULL`.
- A nullable column's patch is `Patch<T>`, not `Patch<Option<T>>`, so `null`
  means "clear".
- `created_at` and `updated_at` are not patchable. The builder writes
  `updated_at` itself.
- A flattened fragment contributes its columns as top-level patch fields.
- The patch derives `garde::Validate`; validation runs before the builder, as
  it does for any other request input.

See [dbkit annotations](dbkit-annotations.md#generated-patch) for the full
patch generation rules.

## Applying patches

```rust
let first = UserPatch {
    id: UserId(Uuid::now_v7()),
    version: 3,
    email: Patch::Set(Email("new@example.com".to_owned())),
    email_verified_at: Patch::Null,
};

let second = UserPatch {
    id: UserId(Uuid::now_v7()),
    version: 7,
    email: Patch::Absent,
    email_verified_at: Patch::Set(Utc::now()),
};

User::patch([first, second]).execute(&pool).await?;
```

The builder is all-or-nothing:

- every entry is rendered before any SQL runs, so a bind error never leaves a
  half-applied batch;
- a pool or plain connection opens a transaction and commits it only after
  every entry succeeded;
- an error drops the transaction, rolling back every earlier entry;
- an empty entry list is a no-op.

### Nested transactions

`execute` takes anything implementing `sqlx::Acquire`. When the caller already
holds a transaction, passing `&mut *transaction` creates a savepoint instead of
a new transaction:

```rust
let mut transaction = pool.begin().await?;

User::patch([first]).execute(&mut *transaction).await?;

// The batch above made and released its savepoint; the outer transaction is
// still open.
User::patch([second]).execute(&mut *transaction).await?;

transaction.commit().await?;
```

A failing nested batch restores its savepoint and leaves the outer transaction
usable. The caller decides whether several batches, or SQL of its own, share one
outer transaction.

### Exactly one row per entry

Each entry must update exactly one row. The builder returns
`PatchError::NotUpdated { index }` when `rows_affected` is not one, which means:

- the row is gone, or
- the version in the patch is stale.

Both outcomes roll the batch back. The index identifies the entry that failed.

## SQL

A patch that sets `email` and clears `email_verified_at` renders:

```sql
UPDATE "user"
SET "email" = $1,
    "email_verified_at" = NULL,
    "version" = "version" + 1,
    "updated_at" = now()
WHERE "id" = $2
  AND "version" = $3
-- params: ["new@example.com", id, 3]
```

Rules:

- The table and column names come from `TableMeta`, never from a request.
- `Patch::Absent` columns are omitted. Two entries with different changed
  columns produce different statements.
- `Patch::Set` binds one placeholder.
- `Patch::Null` writes literal `NULL` and binds nothing, so it does not advance
  placeholder numbering.
- The primary key is the `WHERE` clause, in declaration order. A composite key
  renders `"lang" = $1 AND "key" = $2`.
- A versioned model adds `"version" = $n` to `WHERE` and
  `"version" = "version" + 1` to `SET`.
- A model with `updated_at` adds `"updated_at" = now()` to `SET`.
- `now()` is the transaction timestamp, so every patch in one transaction sees
  the same update time.

Parameters are ordered as they appear in the statement: changed values first,
then primary key values, then the expected version.

## Rendering without executing

`render()` returns one `RenderedPatch` per entry, for assertions and logs:

```rust
let rendered = User::patch([first]).render()?;

assert_eq!(
    rendered[0].sql,
    "UPDATE \"user\" SET \"email\" = $1, \"email_verified_at\" = NULL, \
     \"version\" = \"version\" + 1, \"updated_at\" = now() \
     WHERE \"id\" = $2 AND \"version\" = $3"
);
assert_eq!(
    rendered[0].values,
    vec![json!("new@example.com"), json!("018f..."), json!(3)]
);
```

- `RenderedPatch::sql` is the statement text.
- `RenderedPatch::arguments` is the assembled `PgArguments`.
- `RenderedPatch::values` is the same values as JSON, in placeholder order.

Rendering does not touch the database. A stale version still renders normally;
the conflict appears only when the statement runs.

## Generated update surface

`#[derive(DbPatch)]` implements `dbkit::PatchRow` for `<Model>Patch`:

```rust
impl PatchRow for UserPatch {
    type Model = User;

    fn assignments(&self) -> Vec<Assignment<'_>>;
    fn key(&self) -> Vec<&dyn Bind>;
    fn version(&self) -> Option<&dyn Bind>;
}
```

- `assignments` lists the changed business columns in declaration order;
  `None` is a cleared column.
- `key` lists primary key values in `TableMeta::primary_key` order.
- `version` is `Some` exactly when the model has a version column.
- A flattened fragment's patch implements the same trait, and the parent
  extends its assignments.
- A fragment patch has no key and no version, so it can be embedded but not
  applied on its own. `::patch()` is generated only for tables.

For a table, the derive also adds:

```rust
impl User {
    pub fn patch(
        entries: impl IntoIterator<Item = UserPatch>,
    ) -> dbkit::PatchQuery<UserPatch> {
        dbkit::PatchQuery::new(entries)
    }
}
```

The patch struct follows the model's visibility. A private model gets a private
patch type.

The patch payload types are exposed as `&dyn Bind`, so a model field must be a
`Bind` value: a database type plus `Encode`, `Serialize`, `PartialEq`, `Debug`,
`Send` and `Sync`. Transparent newtypes get this from `#[derive(sqlx::Type)]`.

## Errors

```rust
pub enum PatchError {
    Empty { index: usize },
    KeyColumns { expected: usize, actual: usize },
    VersionColumns,
    NotATable,
    NotUpdated { index: usize },
    Sqlx(sqlx::Error),
}
```

- `Empty` — an entry has no changed business column. It is rejected rather than
  bumping the version and `updated_at` for no reason.
- `KeyColumns` — the generated key value count disagrees with
  `TableMeta::primary_key`. This is a code-generation guard, not a request
  error.
- `VersionColumns` — the model has a version but the patch does not, or the
  reverse. Also a code-generation guard.
- `NotATable` — the model is a fragment or subquery. Generated table patches
  cannot reach this.
- `NotUpdated` — an entry affected zero or more than one row.
- `Sqlx` — encoding, execution, savepoint, or commit failed.

## What the builder does not do

- It does not validate. Call `patch.validate()` before building a batch.
- It does not insert rows.
- It does not return updated rows. The result is success or an error; a
  `RETURNING` variant could be added separately.
- It does not batch different models. Compose several `::patch(...)` calls in
  one outer transaction instead.
- It does not order entries or manage deadlocks. Entries run in input order.
- It does not make version bumping or `updated_at` optional. Every update bumps
  the version once and refreshes `updated_at`.
- It does not distinguish a missing row from a stale version. Both are
  `NotUpdated`; a follow-up `SELECT` would be needed to tell them apart.
