# Delete builder

A `#[derive(DbDelete)]` model gets a sibling of `::insert()`, `::query()` and
`::patch()`:

```rust
User::delete([
    UserPKV { id: user_id, version: 3 },
    UserPKV { id: other_id, version: 7 },
])
.execute(&pool)
.await?;
```

`::delete(entries)` builds a `DeleteQuery<UserPKV>`, renders one `DELETE` per
entry, and runs them in one transaction. Each entry names its row by primary
key and, when the model has a version, checks the version it expects.

The input is the key struct `DbModel` already generates; there is no
`UserDelete` type.

## The key input

`DbModel` generates `{Model}PK` for inserts and `{Model}PKV` for operations that
identify a row and check its version:

```rust
pub struct UserPK {
    pub id: UserId,
}

pub struct UserPKV {
    pub id: UserId,
    pub version: i64,
}
```

- `DeleteKey` is implemented for `{Model}PKV`, not for `{Model}PK`.
- When the model has a `#[db(version)]` field, `PKV` appends it, so every
  delete states the version it expects.
- When the model has no version, `PKV` is an alias of `PK`, so deletion is by
  primary key alone.
- A composite key contributes one field per key column, in declaration order.
- Key fields are exposed as `&dyn dbkit::Bind`, so a field type must be a
  database value: `Type + Encode + Serialize + PartialEq + Debug`, which a
  transparent `#[derive(sqlx::Type)]` newtype already is.

A `#[db(fragment)]` or `#[db(subquery = "...")]` model has no table and cannot
derive `DbDelete`.

## Applying deletes

```rust
User::delete([
    UserPKV { id: first, version: 3 },
    UserPKV { id: second, version: 7 },
])
.execute(&pool)
.await?;
```

The builder is all-or-nothing:

- every entry is rendered before any SQL runs, so a bind error never leaves a
  half-applied batch;
- a pool or plain connection opens a transaction and commits it only after
  every entry succeeded;
- an error drops the transaction, rolling back every earlier entry;
- an empty entry list is a no-op.

### Nested transactions

`execute` takes anything implementing `sqlx::Acquire`. Passing
`&mut *transaction` creates a savepoint instead of a new transaction:

```rust
let mut transaction = pool.begin().await?;

User::delete([first]).execute(&mut *transaction).await?;
User::delete([second]).execute(&mut *transaction).await?;

transaction.commit().await?;
```

A failing nested batch restores its savepoint and leaves the outer transaction
usable.

### Exactly one row per entry

Each entry must delete exactly one row. If `rows_affected` is not one, the
builder returns `DeleteError::NotDeleted { index }` and rolls the batch back.

For example, a four-entry batch where the fourth key is missing:

- entries 1–3 delete their rows;
- entry 4 deletes no row and returns `NotDeleted { index: 3 }`;
- the transaction is dropped, so the first three rows are restored;
- `execute` returns `Err`, never `Ok` after a partial delete.

The same happens when a versioned row changed since the key was read: the row
is not deleted and the batch rolls back. The index identifies the entry that
did not delete exactly one row.

A key that appears twice in one batch therefore fails the batch: the first
entry deletes the row and the second deletes nothing. Entries are not
deduplicated.

## SQL

A versioned key renders:

```sql
DELETE FROM "user"
WHERE "id" = $1
  AND "version" = $2
-- params: [id, 3]
```

An unversioned key renders only the key predicate:

```sql
DELETE FROM "note"
WHERE "id" = $1
-- params: [id]
```

Rules:

- The table and column names come from `TableMeta`, never from a request.
- The primary key is the `WHERE` clause, in declaration order. A composite key
  renders `"lang" = $1 AND "key" = $2`.
- A versioned model appends `"version" = $n` to `WHERE`.
- Every entry is its own `DELETE`, so the batch preserves input order and a
  failed entry can be named by index. A single multi-row statement could check
  the total row count, but a composite key plus version makes the predicate
  awkward and the error could not name the offending entry.

## Rendering without executing

`render()` returns one `RenderedDelete` per entry, for assertions and logs:

```rust
let rendered = User::delete([UserPKV { id, version: 3 }]).render()?;

assert_eq!(
    rendered[0].sql,
    "DELETE FROM \"user\" WHERE \"id\" = $1 AND \"version\" = $2"
);
assert_eq!(rendered[0].values, vec![json!(id), json!(3)]);
```

- `RenderedDelete::sql` is the statement text.
- `RenderedDelete::arguments` is the assembled `PgArguments`.
- `RenderedDelete::values` is the same values as JSON, in placeholder order.

Rendering does not touch the database. A stale version still renders normally;
the conflict appears only when the statement runs.

## Generated delete surface

`#[derive(DbDelete)]` implements `dbkit::DeleteKey` for `{Model}PKV`:

```rust
impl DeleteKey for UserPKV {
    type Model = User;

    fn key(&self) -> Vec<&dyn Bind>;
    fn version(&self) -> Option<&dyn Bind>;
}
```

- `key` lists primary key values in `TableMeta::primary_key` order.
- `version` is `Some` exactly when the model has a version column; for an
  unversioned model `PKV` is `PK` and this is always `None`.
- A fragment or a subquery cannot derive it, so `::delete()` is generated only
  for tables.

The derive also adds:

```rust
impl User {
    pub fn delete(
        entries: impl IntoIterator<Item = UserPKV>,
    ) -> dbkit::DeleteQuery<UserPKV> {
        dbkit::DeleteQuery::new(entries)
    }
}
```

The generated query type mirrors `PatchQuery`:

```rust
pub struct DeleteQuery<K: DeleteKey> {
    // private
}

impl<K: DeleteKey> DeleteQuery<K> {
    pub fn new(entries: impl IntoIterator<Item = K>) -> Self;
    pub fn render(&self) -> Result<Vec<RenderedDelete>, DeleteError>;

    pub async fn execute<'c, A>(
        self,
        acquirer: A,
    ) -> Result<(), DeleteError>
    where
        A: Acquire<'c, Database = Postgres>;
}
```

## Errors

```rust
pub enum DeleteError {
    KeyColumns { expected: usize, actual: usize },
    VersionColumns,
    NotATable,
    NotDeleted { index: usize },
    Sqlx(sqlx::Error),
}
```

- `KeyColumns` — the generated key value count disagrees with
  `TableMeta::primary_key`. This is a code-generation guard, not a request
  error.
- `VersionColumns` — the model has a version but the key does not, or the
  reverse. Also a code-generation guard.
- `NotATable` — the model is a fragment or subquery. Generated table deletes
  cannot reach this.
- `NotDeleted` — an entry affected zero or more than one row. The batch is
  rolled back.
- `Sqlx` — encoding, execution, savepoint or commit failed. Foreign-key and
  other constraint violations stay wrapped here; a handler inspects
  `sqlx::Error::Database` to map a constraint to a status.

## What the builder does not do

- It does not soft-delete. A `deleted_at` column is an ordinary nullable column;
  clear or set it with `::patch()` if the table uses soft deletion.
- It does not delete by filter. `::query()` is read-only; the delete builder
  takes keys only.
- It does not cascade. PostgreSQL does whatever the schema declares.
- It does not return the deleted rows. The result is success or an error; a
  `RETURNING` variant could be added separately.
- It does not deduplicate keys. A duplicate entry fails the batch.
- It does not batch different models. Compose several `::delete(...)` calls in
  one outer transaction instead.
- It does not distinguish a missing row from a stale version. Both are
  `NotDeleted`; a follow-up `SELECT` would be needed to tell them apart.
