# dbkit annotations

Annotations understood by `#[derive(DbModel)]` from the `dbkit-macros` crate.
They describe the fixed columns of a table row: which field is the primary key,
which one is the optimistic-locking token, and which ones are audit columns.

The derive currently **only validates** the annotations and generates nothing.
A struct that carries valid annotations compiles; the `Model`, `Patchable` and
insert implementations do not exist yet, so later sections describe the
contract the annotations are meant to feed rather than code you can call
today. When code generation lands, the accepted syntax stays as documented
here and only the emitted code is new.

## Example

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, DbModel)]
#[db(table = "email")]
pub struct Email {
    #[db(id)]
    pub id: EmailId,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub sent_at: Option<DateTime<Utc>>,
    #[db(created_at)]
    pub created_at: DateTime<Utc>,
    #[db(updated_at)]
    pub updated_at: DateTime<Utc>,
    #[db(version)]
    pub version: i64,
}
```

Fields without an annotation are ordinary business columns. They are the
fields a generated patch would expose; annotated fields never are.

## Where annotations may appear

- `#[db(table = "...")]` on the struct, exactly once.
- `#[db(id)]`, `#[db(version)]`, `#[db(created_at)]`, `#[db(updated_at)]` on a
  field.
- Multiple `#[db(...)]` attributes on one field are allowed; the roles are
  collected across all of them.
- All attributes in the accepted vocabulary are listed here. Anything else is a
  compile error rather than an ignored annotation, so a typo never silently
  changes behaviour.

## `table`

Names the table the row belongs to.

- Legal on: the struct, exactly once.
- Value: a non-empty string literal.
- Missing, empty, or repeated `table` is an error.
- The value is the plain identifier without quotes or a schema prefix;
  generated SQL quotes it as `"..."`.

## `id`

Marks the primary key field.

- Legal on: exactly one field, required.
- The field must not be `Option`.
- Any type is accepted; it becomes the `Model::Id` associated type, so it must
  encode to the database column by itself.
- Used for the `WHERE` clause of single-row operations and for `find`.

## `version`

Marks the optimistic-locking field.

- Legal on: exactly one field, required.
- The field must be `i64` and must not be `Option`.
- Reads produce `Model::version()`, writes append the guard
  `"version" = "version" + 1` and the condition `WHERE "version" = $n`.
- A guard miss surfaces as `RepoError::Conflict`, not as a generic database
  error.

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
- The struct must carry `#[db(table = "...")]`.
- `table` must be a non-empty string literal and appear only once.
- `id` must be present exactly once.
- `version` must be present exactly once.
- `created_at` and `updated_at` may each appear at most once.
- A single field may carry at most one role, even if the roles are split across
  several `#[db(...)]` attributes.
- A role must not be applied to an `Option` field.
- `version` must be `i64`.
- `created_at` and `updated_at` must be `DateTime`.
- An empty `#[db]` attribute is an error.
- The struct-level attribute accepts only `table`; the field-level attribute
  accepts only the four roles.

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
the role a field plays for the generated code.

## Error messages

For reference when reading a compile failure:

- `` `DbModel` can only be derived for structs ``
- `` `DbModel` requires named fields ``
- `` missing `#[db(table = "...")]` ``
- ``unknown attribute; the struct only accepts `table` ``
- `` `table` must not be empty ``
- `` duplicate `table` ``
- ``unknown attribute; expected `id`, `version`, `created_at` or `updated_at` ``
- ``empty `#[db]` attribute``
- ``a field can carry at most one `#[db(...)]` role``
- `` missing `#[db(id)]` `` and `` missing `#[db(version)]` ``
- ``duplicate `#[db(...)]` role``
- `` `#[db(id)]` field must not be `Option` `` (and the same for each other role)
- `` `#[db(version)]` field must be `i64` ``
- `` `#[db(created_at)]` field must be a `DateTime` ``

All problems found in one struct are reported together, so a single compile
fixes the whole annotation block.

## Relation to the schema test

The `COLUMNS` constant of a `Model` and the annotated struct must stay in sync
with `schema.sql`; the per-service `tests/schema.rs` compares the column set
with `information_schema`. When code generation lands, that comparison moves
from a hand-written `COLUMNS` to a generated one, and the test keeps its job:
catching a table that the model does not describe.

## Not yet supported

These are deliberately out of scope until a model needs them:

- Soft-delete columns (`deleted_at`) and any read filter convention around
  them.
- Column names that differ from field names.
- Non-`i64` version types.
- Tables without a version column.
- Multi-column primary keys.
