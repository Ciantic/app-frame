# Insert builder

A `#[derive(DbInsert)]` model gets a sibling of `::query()` and `::patch()`:

```rust
let user = UserInsert::new(Email("new@example.com".to_owned()))
    .set_email_verified_at(Utc::now());

let inserted: Vec<User> = User::insert([user]).execute(&pool).await?;
```

`::insert(entries)` builds an `InsertQuery<UserInsert>`, renders one `INSERT`
per entry, and returns the inserted rows. Each entry may omit the columns the
database can fill.

## The insert input

A model's insert separates columns the caller must supply from columns the
database may fill.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, DbModel, DbInsert)]
#[db(table = "user")]
pub struct User {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: UserId,

    #[db(pgtype = "text")]
    pub email: Email,

    #[db(pgtype = "timestamptz")]
    pub email_verified_at: Option<DateTime<Utc>>,

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

// generated
pub struct UserInsert {
    pub id: Option<UserId>,
    pub email: Email,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub version: Option<i64>,
}
```

- A **required** column is non-null and has no default. Its field is plain `T`.
- An **optional** column is nullable or has a default. Its field is `Option<T>`.
  `None` omits the column from the `INSERT`, so PostgreSQL applies the default
  or stores `NULL`.
- A nullable column is optional even without a default: omitting it stores
  `NULL`.
- A defaulted column is optional even when it is not nullable: omitting it lets
  the database fill it in.
- A composite key is classified one column at a time. A defaulted key column is
  optional; one without a default is required.

### Defaults

- `#[db(pgdefault = "now()")]` declares a default on the field.
- `#[db(pgdefault = inherit)]` takes the field type's `DbType::PG_DEFAULT`, even
  when the field declares its own `pgtype`.
- Omitting `pgdefault` means no default, regardless of `pgtype`.
- `pgtype` and `pgdefault` are independent: `pgtype` decides the column type,
  `pgdefault` decides the default.
- Bare `#[db(pgdefault)]`, `pgdefault = none` and unknown identifiers are errors.
- For a non-null inherited default, the derive emits a const assertion that
  `DbType::PG_DEFAULT` is `Some`. A type without a default is a compile error:
  remove `inherit` to declare no default, or give the type a default.
- A nullable inherited field needs no assertion: omission is valid whether the
  type has a default or not.

The common table shape therefore opts in explicitly:

```rust
#[derive(DbType, sqlx::Type)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

#[db(primary_key)]
#[db(pgdefault = inherit)]
pub id: UserId,
```

### Constructor and setters

```rust
impl UserInsert {
    pub fn new(email: impl Into<Email>) -> Self;
    pub fn set_email(mut self, value: impl Into<Email>) -> Self;
    pub fn set_id(mut self, value: impl Into<UserId>) -> Self;
    pub fn set_email_verified_at(mut self, value: DateTime<Utc>) -> Self;
    pub fn set_created_at(mut self, value: DateTime<Utc>) -> Self;
    pub fn set_updated_at(mut self, value: DateTime<Utc>) -> Self;
    pub fn set_version(mut self, value: i64) -> Self;
}
```

- `new` takes the required fields in declaration order. A model with no required
  fields gets `new()` with no arguments.
- Optional fields start as `None`.
- `set_<field>(value)` sets an optional field to `Some(value)` or replaces a
  required field.
- There is no `clear_<field>` method. “Not given” on insert means the database
  chooses; it is not the same as SQL `NULL`.
- A nullable column without a default is cleared by leaving it `None`.

### Serialization and validation

- The struct derives `Serialize`, `Deserialize` and `garde::Validate`.
- Optional fields carry `#[serde(default, skip_serializing_if = "Option::is_none")]`,
  so a missing key is `None` and never reaches the wire as `null`.
- A required field has no serde default: a missing key is a deserialization
  error.
- Business fields whose payload type can carry `garde` rules are marked
  `#[garde(dive)]`. Role fields (primary key, version, timestamps) are skipped.
- A handler deserializes the insert, calls `validate()`, then passes it to the
  builder.

### Flattened fragments

- `DbInsert` is also derived for a `#[db(fragment)]`, producing
  `{Fragment}Insert`. A fragment insert cannot be applied on its own, but it is
  a valid building block.
- The parent field is `#[serde(flatten)] pub size: SizeInsert`.
- The parent constructor takes each flattened insert as an argument, because a
  fragment may contain required columns.
- A flattened field also gets `set_<field>({Fragment}Insert)` for replacing the
  whole fragment insert.

```rust
let size = SizeInsert::new(640).set_note("wide");
let frame = FrameInsert::new(id, size, "photo");
```

## Applying inserts

```rust
let first = UserInsert::new(Email("one@example.com".to_owned()));
let second = UserInsert::new(Email("two@example.com".to_owned()))
    .set_email_verified_at(Utc::now());

User::insert([first, second]).execute(&pool).await?;
```

- One row is `User::insert([user])`; several are an array or a `Vec`. Rust has no
  variadic methods, so `insert` takes any `IntoIterator`.
- `execute` renders every entry before opening a transaction, so a bind error
  never leaves a half-applied batch.
- A pool or plain connection opens a transaction and commits after every entry
  succeeded. An error drops the transaction and rolls back every earlier entry.
- An empty entry list is a no-op.
- `execute` returns `Vec<User>` in input order, including the values the
  database generated.

### Nested transactions

```rust
let mut transaction = pool.begin().await?;

User::insert([first]).execute(&mut *transaction).await?;
User::insert([second]).execute(&mut *transaction).await?;

transaction.commit().await?;
```

A `&mut Transaction` makes the batch use a savepoint. A failing nested batch
restores its savepoint and leaves the outer transaction usable.

## SQL

A required column renders its value:

```sql
INSERT INTO "user" ("email")
VALUES ($1)
RETURNING *
-- params: ["one@example.com"]
```

A nullable column left `None` is omitted; setting it adds a placeholder:

```sql
INSERT INTO "user" ("email", "email_verified_at")
VALUES ($1, $2)
RETURNING *
```

An entry with every column optional and every field `None` renders:

```sql
INSERT INTO "user"
DEFAULT VALUES
RETURNING *
```

Import values are explicit:

```sql
INSERT INTO "user" ("email", "created_at", "updated_at", "version")
VALUES ($1, $2, $3, $4)
RETURNING *
```

- The table and column names come from `TableMeta`, never from a request.
- Columns appear in model declaration order, with flattened columns spliced in
  place. A synthesized `updated_at` keeps its declaration position.
- Only provided columns appear. Omitted defaulted columns are left to the
  database.
- `RETURNING *` decodes into the model, so the caller sees generated ids,
  timestamps and versions.
- Every entry is its own `INSERT`, because entries can provide different
  optional columns and a single multi-row statement would need one uniform
  column list.

### Imported timestamps

For an import where `created_at` is supplied but `updated_at` is not, the
renderer also writes `updated_at` with the supplied creation time. An imported
row then does not look freshly modified. The two timestamps are bound as two
parameters.

## Errors

```rust
pub enum InsertError {
    NotATable,
    NotInserted { index: usize },
    Sqlx(sqlx::Error),
}
```

- `NotATable` — the model is a fragment or a subquery. The generated table
  method cannot reach it.
- `NotInserted` — an `INSERT ... RETURNING *` returned no row.
- `Sqlx` — encoding, execution, decoding, savepoint or commit failed. Unique,
  foreign-key, check and not-null violations stay wrapped here; a handler
  inspects `sqlx::Error::Database` to map a constraint to a status.

There is no `Empty` variant: `DEFAULT VALUES` is a valid insert, and an empty
batch is a no-op.

## What the builder does not do

- It does not validate. Call `insert.validate()` before building a batch.
- It does not upsert. `ON CONFLICT` is not generated.
- It does not select a partial `RETURNING`; it always returns the whole model.
- It does not combine compatible entries into one multi-row `INSERT`.
- It does not evaluate defaults in Rust. PostgreSQL evaluates `pgdefault`.
- It does not force a value on a nullable defaulted column. `None` applies the
  default; there is no explicit-`NULL` state in the first version.
