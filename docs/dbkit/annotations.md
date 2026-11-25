# dbkit annotations

Annotations understood by `#[derive(DbModel)]` from the `dbkit-macros` crate.
They describe the fixed columns of a table row: which fields form the primary
key, which one carries the optimistic-locking token, which ones are audit
columns, which columns the database fills in from a default, which comparisons
each column may be queried with, and whether it may be sorted. They also
describe related non-column fields, foreign keys, unique keys, trigger
definitions and generated columns. Every field carries a `pgtype` unless it is
flattened, is a relation, or its type declares one with `#[derive(DbType)]`, and a table
row needs a primary key; the other annotations are opt-in.

A struct is one of three shapes:

- A table row, `#[db(table = "...")]`, which generates `impl dbkit::Model`
  with `Model::meta()`.
- A subquery row, `#[db(subquery = "...")]`, whose row source is a query
  rather than a table. It generates `impl dbkit::Model` returning
  `Meta::SubQuery(&SubQuery { sql, columns })` and is read-only.
- A fragment, `#[db(fragment)]`, which is a group of columns with no table of
  its own. It generates `impl dbkit::Model` returning
  `Meta::Fragment(&TableFragment { columns })`, and another model embeds it
  with `#[db(flatten)]`.

`#[derive(DbPatch)]` and `#[derive(DbInsert)]` read the same annotations to
generate the model's patch and insert input types, which `garde` validates.
`#[derive(DbDelete)]` reads them to link the generated key struct to a batch
delete. [Patch builder](patch-builder.md), [Insert builder](insert-builder.md)
and [Delete builder](delete-builder.md) document the generated queries. The
same `#[derive(DbModel)]` expansion also generates the model's selectable
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
    #[db(query_ops(eq, neq, like, ilike, any))]
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
generated patch would expose unless they are `pg_virtual`, trigger-maintained
or a relation; role-annotated fields never are.

## Where annotations may appear

- `#[db(table = "...")]`, `#[db(subquery = "...")]` or `#[db(fragment)]` on
  the struct, exactly once. They are mutually exclusive; `table` makes the
  struct a table row, `subquery` makes its rows the output of a query, and
  `fragment` makes it a column group.
- `#[db(pg_trigger = "...")]` or `#[db(pg_trigger(...))]` on a table row or a
  scalar field of a table row, repeatable; `#[db(unique)]` on a field and
  `#[db(unique(columns = [...]))]` on a table row.
- `#[db(pgtype = "...")]` on every field that is not `#[db(flatten)]`,
  `#[db(relation)]`, unless the field type implements `dbkit::DbType`, exactly
  once.
- `#[db(pgdefault = "...")]` on a field, at most once.
- `#[db(pgtype = "...")]` and `#[db(pgdefault = "...")]` on the struct of a
  value type that derives `DbType`, instead of on a field.
- `#[db(primary_key)]`, `#[db(version)]`, `#[db(created_at)]`,
  `#[db(updated_at)]`, `#[db(tenant)]` on a field of a table row.
- `#[db(query_ops(...))]` on a field, at most once; the parentheses hold a
  comma-separated operator list.
- `#[db(orderable)]` on a field, at most once.
- `#[db(pg_virtual = "...")]`, `#[db(unique)]`, `#[db(foreign_key(...))]` or
  `#[db(relation(...))]` on a field, at most once each.
- `#[db(pg_trigger(...))]` on a field may hold a full definition or reference a
  trigger declared elsewhere; see [`pg_trigger`](#pg_trigger).
- `#[db(flatten)]` on a field of a table row or a subquery row, at most once.
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
- Role first, then `pgtype`, then `pgdefault`, then `query_ops`, then
  `orderable`, then `pg_trigger`, then `pg_virtual`, `unique`, `foreign_key` or
  `relation`: `#[db(primary_key)]` above `#[db(pgtype = "uuid")]` above
  `#[db(pgdefault = "uuidv7()")]` above `#[db(query_ops(eq, any))]` above
  `#[db(orderable)]` above `#[db(pg_trigger = "...")]`.
- Struct-level `pg_trigger` and `unique(columns = [...])` follow the shape
  annotation.
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

## `subquery`

Makes the struct's rows the output of a query, read as if they were a table.

- Legal on: the struct, exactly once, as an alternative to `table` and
  `fragment`.
- Value: a non-empty string literal holding a `SELECT`, optionally preceded by
  a `WITH` clause.
- The value is not quoted or rewritten. A query of the model renders it as a
  derived table, `SELECT ... FROM (<sql>) AS "subquery"`, so projections,
  filters and paging apply to the query's output columns. PostgreSQL allows a
  `WITH` clause inside a parenthesized `FROM` item, so a CTE takes the same
  annotation.
- Write the SQL without a trailing semicolon: the string is a derived table,
  not a statement.
- The SQL carries no bound parameters: the builder numbers its own filters
  from `$1`, so a placeholder in the annotation would collide.
- The struct declares the columns the query returns. It has no primary key,
  version or audit columns, and role annotations are rejected.
- `pgtype`, `query_ops` and `orderable` per field work as usual.
- A subquery row is read-only: `#[derive(DbPatch)]` rejects it.
- A subquery cannot have generic parameters, because the metadata cache is one
  `static` per type.
- Subqueries are not listed in `MODELS`, because they are not tables.

```rust
#[derive(DbModel, DbSelect, sqlx::FromRow)]
#[db(subquery = "SELECT status, max(total) AS total FROM orders GROUP BY status")]
pub struct StatusTotal {
    #[db(pgtype = "varchar(20)")]
    #[db(query_ops(eq))]
    pub status: String,

    #[db(pgtype = "int8")]
    #[db(query_ops(gte))]
    pub total: i64,
}
```

`StatusTotal::query()` renders `SELECT * FROM (SELECT status, max(total) AS
total FROM orders GROUP BY status) AS "subquery"`, and a filter on `total`
becomes a `WHERE` on the aggregate.

## `fragment`

Makes the struct a group of columns instead of a table row.

- Legal on: the struct, exactly once, as an alternative to `table`.
- Value: none; it is a bare annotation.
- A fragment has no table name, no primary key and no roles. `pgtype` is
  required on every field, and `pgdefault`, `query_ops` and `orderable` work
  as usual.
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

Splices the columns of a fragment into the enclosing table or subquery.

- Legal on: a field of a table row or a subquery row, at most once.
- Value: none; it is a bare annotation.
- The field type must derive `DbModel` with `#[db(fragment)]`. The derive
  cannot inspect the type, so it matches on `<Type as dbkit::Model>::meta()`
  and, on `Meta::Fragment`, extends the column list with its `columns`. A type
  that does not derive `DbModel` fails to compile; a table or a subquery panics
  on the first `meta()` call, naming the field and what it found.
- The fragment's columns take the field's position in the enclosing column
  list, so declaration order is preserved across plain and flattened fields.
- A flattened field is not a column itself: it must not carry `pgtype`,
  `pgdefault`, `query_ops`, `orderable` or a role, and it must not be
  `Option`. An optional group is expressed by nullable columns inside the
  fragment.
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

Moves the column type and its default to the value type, so a model field can
take the type from it and opt into the default with
`#[db(pgdefault = inherit)]`.

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
    #[db(pgdefault = inherit)]
    pub id: UserSessionId,

    // `UserId` declares a default too, but this column has none.
    pub user_id: UserId,

    // ...
}
```

- `#[db(pgtype = "...")]` on the struct is required and takes the same values
  as a field's `pgtype`; `#[db(pgdefault = "...")]` is optional.
- Generates `impl dbkit::DbType`, whose `PG_TYPE` and `PG_DEFAULT` constants the
  model derive reads. A field that omits `pgtype` takes its type from the type;
  `Option<T>` inherits from `T` and stays nullable.
- A field inherits the type's default only with `#[db(pgdefault = inherit)]`.
  Omitting `pgdefault` means no default, regardless of whether the field
  declares `pgtype`.
- This is why plain types such as `DateTime<Utc>` and `String` need no `DbType`
  implementation: their fields spell out `pgtype` and declare no default.
- The type-level default is a suggestion: the same value type is used for
  foreign keys, whose columns have no default. Such a field omits `pgdefault`.
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
- On a field, the value is either `inherit` or a non-empty string literal:
  - `#[db(pgdefault = inherit)]` takes the default from the field type's
    `DbType::PG_DEFAULT`, even when the field declares its own `pgtype`;
  - a string holds the default expression: `uuidv7()`, `now()`, `0`,
    `'{}'::jsonb`.
- Omitting `pgdefault` on a field means the column has no default, regardless
  of whether the field declares `pgtype`.
- On a `DbType` struct, the value is a non-empty string literal or omitted; the
  keyword forms are field-only.
- An empty, repeated, bare or unknown `pgdefault` is an error.
- `inherit` is the only way to use a type-level default. The schema test and
  `DbModel` read `DbType::PG_DEFAULT` for the column; a non-null insert field
  also carries a compile-time assertion that the type really has a default.
- Meant to mirror the database rather than change it: the model does not emit
  the default. It tells `DbInsert` that the column can be omitted and lets the
  schema test detect drift.
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
  `#[db(query_ops(eq, neq, like, any))]`. A bare `#[db(eq)]` is not accepted,
  and an empty `query_ops()` is an error.
- Operators: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `like`, `ilike`, `not_like`,
  `not_ilike`, `any`, `not_any`, `contains`, `not_contains`, `overlaps`,
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

## `orderable`

Allows the column in `ORDER BY`.

- Legal on: a field, optional and at most once; a bare annotation.
- The derive implements the `dbkit::HasOrder` capability for the column's tag
  type, so `c.column.asc()` and `c.column.desc()` exist only on a declared
  column; an undeclared one is a compile error. The flag is also recorded in
  `Column::orderable`, which the JSON deserializer and `render()` check, so a
  request cannot order by a column the model did not allow.
- Independence: ordering needs no `query_ops` entry, and `query_ops` does not
  imply ordering. A column can be sortable but not filterable, which is what a
  denormalized display column usually wants.
- Not checked against the column type: PostgreSQL accepts any type in
  `ORDER BY`, including `jsonb` and arrays, so the annotation is the only gate.
- A `#[db(flatten)]` field forwards its columns' flags, so a fragment's
  orderable column stays orderable in the parent.

```rust
#[derive(DbModel)]
#[db(table = "orders")]
pub struct Order {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: Uuid,

    #[db(pgtype = "int8")]
    #[db(query_ops(eq, gte, lt))]
    #[db(orderable)]
    pub total: i64,
}
```

`Order::query().order_by([c.total.desc()])` renders
`SELECT * FROM "orders" ORDER BY "total" DESC`; `c.id.asc()` does not compile.

## `primary_key`

Marks a primary key column. Repeat it on every column of a composite key.

- Legal on: one or more fields, required.
- A field must not be `Option`.
- Any type is accepted; it must encode to the database column by itself.
- A single annotated field is the `Model::Id` associated type; with several, the
  associated type is a tuple of the field types in declaration order.
- Used for the `WHERE` clause of single-row operations and for `find`.
- A required field of the generated patch. In the generated insert it is
  required only when the column has no default; a defaulted key is optional.

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
- In the generated insert it is the starting version: optional when the column
  has a default, required otherwise.

## `created_at`

Marks the creation timestamp.

- Legal on: at most one field, optional. A model without it gets no creation
  timestamp handling.
- The field must be a `DateTime` and must not be `Option`.
- Not patchable: the update path never emits it.
- In the generated insert it is optional when the column has a default and
  required when it does not. `Some` copies an existing value, which is what
  importing old rows needs. If `created_at` is supplied and `updated_at` is not,
  the insert also writes `updated_at` with the creation time.

## `updated_at`

Marks the modification timestamp.

- Legal on: at most one field, optional.
- The field must be a `DateTime` and must not be `Option`.
- Not patchable: the update path always writes it from `now()`, ignoring any
  value in the row.
- In the generated insert it is optional when the column has a default and
  required when it does not. When `created_at` is supplied and `updated_at` is
  omitted, the builder writes the creation timestamp to both, so an imported row
  does not look freshly modified.

## `tenant`

Marks the tenant ownership column.

- Legal on: one field of a table row, optional; not on a fragment or a
  subquery.
- The field must not be `Option` and must not have a default, so every write
  states its tenant explicitly.
- The field may also carry `#[db(primary_key)]`. In that case the primary key
  predicate already constrains the tenant and the patch and delete renderers
  do not add it again.
- `TableMeta::tenant` records the column name, and `#[derive(DbModel)]`
  implements `dbkit::TenantOwned` with `TENANT_COLUMN`.
- The insert input keeps the tenant column as a required plain field.
- The patch input makes it a required plain field, like the primary key and
  version; it is not `Patch<T>` and cannot be changed. The renderer adds
  `AND "tenant_id" = $n` to `WHERE`.
- `<Model>PKV` includes the tenant column, and the delete renderer adds the
  same predicate.
- Generated insert, patch and key types implement `dbkit::TenantRow`, which the
  HTTP layer uses to choose tenant or global permission per entry.

## `pg_trigger`

Declares a trigger and, when the annotation has a `body`, its full definition.
A trigger is installed on a table; that table is the declaring model's table
unless `on = "..."` names another one. The annotation may sit on the struct or
on the scalar field the trigger fills.

```rust
#[db(table = "invoice")]
pub struct Invoice {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: InvoiceId,

    #[db(pg_trigger(
        name = "invoice_row_net_amount",
        on = "invoice_row",
        timing = after,
        events(insert, update, delete),
        body = r#"begin
            update "invoice" set "net_amount" = (...)
            where "id" in (old."invoice_id", new."invoice_id");
            return null;
        end;"#
    ))]
    #[db(pgdefault = inherit)]
    pub net_amount: Money,

    #[db(pg_trigger = "invoice_row_net_amount")]
    #[db(pgdefault = inherit)]
    pub tax_amount: Money,
}
```

- Legal on: the struct of a table row, or a scalar field of a table row.
  Repeatable. Not legal on a fragment or a subquery.
- Two forms:
  - `#[db(pg_trigger = "name")]` declares that a trigger named `name` exists
    on the model's table. It is a name-only reference.
  - `#[db(pg_trigger(...))]` names a trigger and optionally defines it.
- Options:
  - `name = "..."` is required and non-empty. Trigger names are unique per
    table, not per schema.
  - `on = "..."` is the table the trigger is installed on; it defaults to the
    model's table. A field on one model may therefore declare a trigger that
    fires on another table.
  - `timing = before|after` is required in a definition.
  - `events(insert, update, delete, truncate)` is required in a definition and
    needs at least one event. The derive records events in canonical order and
    rejects repeats.
  - `level = row|statement` defaults to `row`. `truncate` requires
    `level = statement`, and a statement-level trigger cannot have a
    `condition`.
  - `condition = "..."` is an optional `WHEN` expression.
  - `function = "..."` is the trigger function's name; it defaults to the
    trigger name.
  - `language = "..."` defaults to `plpgsql`.
  - `body = "..."` is the function body as a plpgsql block, including
    `begin`/`end` and `return`. Without `body`, the nested form is a reference
    and accepts only `name` and `on`.
- Definitions are deduplicated by `(on, name)`. Repeating one with identical
  fields is fine; repeating it with different fields is a compile error.
- Any field carrying `pg_trigger`, definition or reference, is
  trigger-maintained: `DbInsert` and `DbPatch` omit it, like
  `#[db(pg_virtual)]`. Reads, filters and projections keep the column. It
  cannot combine with a role, `pg_virtual`, `flatten` or `relation`.
- `pgdefault`, `query_ops`, `orderable`, `unique` and `foreign_key` still work
  on the same field.
- The schema test collects declarations from every model, groups them by
  `(table, name)`, and compares names in both directions. When a definition
  exists, it also compares timing, events, level, function, language and body;
  the `condition` is parsed by PostgreSQL on a temporary trigger and compared
  as a tree. Internal constraint triggers and foreign-key enforcement triggers
  are ignored.
- A trigger declared on a table that is not in `MODELS` cannot be checked; the
  schema test reports it.
- Full schema generation is not implemented yet; the fields are recorded so a
  generator can emit `CREATE FUNCTION` and `CREATE TRIGGER`.

## `foreign_key`

Declares a foreign key for metadata and the schema test; the constraint itself
stays in `schema.sql`.

```rust
#[db(foreign_key(table = "customer", column = "id"))]
pub customer_id: Option<CustomerId>,

#[db(foreign_key(table = "customer", column = "id", scope = "tenant_id"))]
pub customer_id: Option<CustomerId>,
```

- Legal on: a scalar field of a table row, at most once. Not legal on
  `#[db(flatten)]`, a fragment or a subquery.
- `table` and `column` are required, non-empty string literals.
- `scope` is optional and names another scalar column of the same table. The
  annotated field must not be the scope column.
- Without `scope`, the local column `(field)` references `table(column)`.
- With `scope`, the local columns `(scope, field)` reference
  `table(scope, column)`. The target scope column has the same name as the
  local one.
- The macro checks `scope` against the model's scalar columns. It cannot check
  the target type; PostgreSQL enforces that.
- The schema test compares the declared columns and target with `pg_constraint`
  in both directions. `on delete` and `on update` actions are not checked.

## `pg_virtual`

Declares a PostgreSQL generated column and marks the field read-only.

```rust
#[db(pg_virtual = "\"net_amount\" + \"tax_amount\"")]
pub total_amount: Money,
```

- Legal on: a scalar field of a table row, at most once. Not legal on
  `#[db(flatten)]`, a fragment or a subquery.
- The value is a non-empty generated-column expression written with the DDL's
  quoted identifiers.
- It cannot combine with a role or with `pgdefault`. A generated primary key or
  version is a different feature.
- `#[derive(DbInsert)]` and `#[derive(DbPatch)]` omit the field; it has no
  setter and no insert field. Reads, filters and projections keep the column.
- The schema test requires the live column to be generated and compares the
  normalized expression. The probe creates a `VIRTUAL` generated column.

## `unique`

Declares a unique constraint or unique index for metadata and the schema test.

```rust
#[db(unique)]
pub number: Option<String>,

#[db(table = "invoice")]
#[db(unique(columns = [tenant_id, id]))]
pub struct Invoice { /* ... */ }
```

- The field form is legal on a scalar field of a table row, at most once. The
  field must not be a primary key, flattened, a fragment or a subquery.
- The struct form is legal on a table row, repeatable. `columns` is a non-empty
  bracketed list of scalar columns. Repeating a column, repeating a key, or
  using exactly the primary key is an error; a scoped key may include a primary
  key column beside the scope column.
- A nullable unique column keeps PostgreSQL's rule that multiple `NULL`s are
  allowed.
- `unique` does not change query, insert or patch inputs.
- The schema test compares `pg_index` unique, non-primary indexes in both
  directions, ignoring partial and expression indexes.

## `relation`

Declares a non-column field that carries another model. The field is skipped by
`FromRow`, the column enum, projections, inserts and patches; it is serialized
when set.

```rust
#[db(relation(
    one,
    table = "customer",
    local_key = [tenant_id, customer_id],
    target_key = [tenant_id, id]
))]
#[sqlx(skip)]
#[serde(default, skip_serializing_if = "Option::is_none")]
pub customer: Option<Customer>,

#[db(relation(
    many,
    table = "invoice_row",
    local_key = [tenant_id, id],
    target_key = [tenant_id, invoice_id]
))]
#[sqlx(skip)]
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub rows: Vec<InvoiceRow>,
```

- Legal on: a table row, at most once. Not legal on a fragment or a subquery.
- Cardinality is explicit: `one` pairs with `Option<T>`, `many` with `Vec<T>`.
  A required `T` is not supported.
- `table` is a required non-empty string literal. `local_key` and `target_key`
  are one or more column names; a single name is a bare identifier, a composite
  key is a bracketed list. Both lists must have the same length.
- Every `local_key` must name a scalar column of this model. The macro cannot
  check `target_key` against the related table.
- A relation is not necessarily a foreign key; it may join on any columns.
  `foreign_key` describes the constraint, `relation` describes the loaded field.
- dbkit does not load relations. A higher layer fills the field after the row
  query.
- The annotation must not combine with `pgtype`, `pgdefault`, `query_ops`,
  `orderable`, a role, `flatten`, `unique`, `foreign_key`, `pg_trigger` or
  `pg_virtual`.
- There is no schema-test counterpart; a relation is not a DDL object.

## Legality rules enforced by the derive

Every rule below produces a compile error.

- The target must be a struct, not an enum or a union.
- The struct must have named fields.
- The struct must carry `#[db(table = "...")]`, `#[db(subquery = "...")]` or
  `#[db(fragment)]`, exactly one of the three.
- `table` and `subquery` must each be a non-empty string literal, and the three
  shapes cannot be combined.
- A subquery must not end with a semicolon.
- A table or a subquery must not have generic parameters.
- A table row must declare `primary_key` at least once; a subquery and a
  fragment must not carry it or any other role.
- `DbPatch` and `DbInsert` cannot be derived for a subquery row.
- Every field that is not `#[db(flatten)]` or `#[db(relation)]` must carry
  `#[db(pgtype = "...")]` or have a type that implements `dbkit::DbType`.
- `#[db(flatten)]` cannot be combined with `pgtype`, `pgdefault`, `query_ops`,
  `orderable`, `pg_trigger`, `pg_virtual`, `unique`, `foreign_key`, `relation`
  or a role, and its field must not be `Option`.
- A fragment cannot `#[db(flatten)]` another struct.
- `#[db(relation)]` is legal only on a table row; its field must be `Option<T>`
  for `one` or `Vec<T>` for `many`, and it cannot combine with a column
  annotation or a role.
- `#[db(pg_trigger)]` is legal only on a table row or a scalar field of a table
  row. A name is required and must be non-empty. A definition also needs
  `timing`, `events(...)` and `body`; a reference accepts only `name` and
  `on`. `truncate` requires `level = statement` and no `condition`;
  a `condition` requires a row-level trigger. Conflicting definitions for the
  same `(on, name)` are an error. A trigger field cannot combine with a role,
  `pg_virtual`, `flatten` or `relation`.
- `#[db(foreign_key)]` is legal only on a scalar field of a table row; `table`
  and `column` are required, and `scope` must name another scalar column.
- `#[db(pg_virtual)]` is legal only on a scalar field of a table row and cannot
  combine with a role or `pgdefault`.
- A field-level `#[db(unique)]` is legal only on a scalar, non-primary-key field
  of a table row. The struct form needs a non-empty `columns` list of scalar
  columns, without repeats, and a key that is not exactly the primary key.
- `version`, `created_at`, `updated_at` and `tenant` may each appear at most
  once.
- A single field may carry at most one role, even if the roles are split across
  several `#[db(...)]` attributes, with one exception: `primary_key` and
  `tenant` may share a field. A primary key column therefore never also takes
  `version`, `created_at` or `updated_at`.
- A role must not be applied to an `Option` field.
- A `tenant` field must not carry `pgdefault`.
- `version` must be `i64`.
- `created_at` and `updated_at` must be `DateTime`.
- An empty `#[db]` attribute is an error.
- `pgtype` must be a non-empty string literal and appear only once per field or
  `DbType` struct. It is not validated at compile time; the schema test runs it
  through PostgreSQL.
- `pgdefault` must be a non-empty string literal or the identifier `inherit`
  on a field, and must appear at most once per field or `DbType` struct. A
  `DbType` struct accepts only the string form. A bare `#[db(pgdefault)]`, an
  unknown identifier, or `pgdefault = none` is an error. It is not validated at
  compile time; the schema test does that.
- `query_ops` appears at most once per field, holds at least one operator, and
  every operator must be in the accepted list; repeating an operator is an
  error.
- `orderable` appears at most once per field and takes no value.
- The `DbModel` struct attribute accepts `table`, `subquery`, `fragment`,
  `pg_trigger` and `unique`; the field-level attribute accepts `pgtype`,
  `pgdefault`, `query_ops`, `orderable`, `pg_trigger`, `pg_virtual`, `unique`,
  `foreign_key`, `relation`, `flatten` and the five roles. The `DbType` struct
  attribute accepts only `pgtype` and `pgdefault`.
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

Annotations mostly do not affect `sqlx::FromRow`, serde or the database
mapping. A model still decodes every returned column, and the annotations
describe the role a field plays for the generated code. `#[db(flatten)]`
splices columns into dbkit's metadata only: a `FromRow` model whose struct
should decode flat columns needs `#[sqlx(flatten)]` on the same field, and its
JSON stays nested unless the field also carries `#[serde(flatten)]`. A
`#[db(relation)]` field is not a column, so it needs `#[sqlx(skip)]`. A
`#[db(pg_virtual)]` field is still decoded, but it is omitted from insert and
patch inputs.

## Error messages

For reference when reading a compile failure:

- `` `DbModel` can only be derived for structs ``
- `` `DbModel` requires named fields ``
- `` missing `#[db(table = "...")]`, `#[db(subquery = "...")]` or `#[db(fragment)]` ``
- ``unknown attribute; the struct accepts `table`, `subquery`, `fragment`, `pg_trigger` or `unique` ``
- `` `table` must not be empty ``
- `` `subquery` must not be empty ``
- `` `subquery` must not end with a semicolon; the query is wrapped in parentheses ``
- `` duplicate `table` ``
- `` duplicate `subquery` ``
- `` duplicate `#[db(fragment)]` ``
- `` `table`, `subquery` and `fragment` are mutually exclusive ``
- `` `DbModel` on a table or a subquery cannot have generic parameters; ... ``
- `` a subquery cannot carry `#[db(...)]` roles ``
- `` `DbPatch` cannot be derived for a `#[db(subquery = "...")]` struct; a subquery is read-only ``
- ``unknown attribute; expected `primary_key`, `version`, `created_at`,
  `updated_at`, `tenant`, `flatten`, `pgtype`, `pgdefault`, `query_ops`,
  `orderable`, `pg_trigger`, `pg_virtual`, `unique`, `foreign_key` or
  `relation` ``
- ``duplicate `orderable` ``
- `` `#[db(flatten)]` cannot be combined with `orderable` ``
- `` a fragment cannot carry `#[db(...)]` roles ``
- `` a fragment cannot `#[db(flatten)]` another struct ``
- `` `#[db(flatten)]` cannot be combined with `pgtype` `` (and the same for
  `pgdefault`, `query_ops`, `orderable`, `pg_trigger`, `pg_virtual`, `unique`,
  `foreign_key`, `relation` and a role)
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
- `` `#[db(tenant)]` field must not have a default ``
- ``unknown query operator; expected `eq`, `neq`, ...``
- ``duplicate query operator``
- `` `#[db(pg_trigger)]` is only legal on a table row ``
- `` `#[db(pg_trigger)]` must not be empty ``
- `` `pg_trigger` requires `name = "..."` ``
- `` `pg_trigger` requires `timing = before` or `timing = after` ``
- `` `pg_trigger` requires `events(...)` ``
- ``unknown `pg_trigger` option; expected `name`, `on`, `timing`, `events`,
  `level`, `condition`, `function`, `language` or `body` ``
- ``a `pg_trigger` without `body` is a reference; it accepts only `name` and
  `on` ``
- ``conflicting `pg_trigger` definitions for `name` on table `table` ``
- `` `truncate` requires `level = statement` ``
- `` `truncate` cannot have a `condition` ``
- `` `condition` is only legal on a row-level trigger ``
- `` `#[db(pg_trigger)]` cannot be combined with `pg_virtual` ``
- `` `#[db(pg_trigger)]` cannot be combined with a role ``
- `` `#[db(flatten)]` cannot be combined with `pg_trigger` ``
- `` `#[db(relation)]` cannot be combined with `pg_trigger` ``
- `` `#[db(pg_virtual)]` is only legal on a table row ``
- `` `#[db(pg_virtual)]` cannot be combined with a role ``
- `` `#[db(pg_virtual)]` cannot be combined with `pgdefault` ``
- `` `#[db(foreign_key)]` is only legal on a table row ``
- `` `foreign_key` requires `table = "..."` `` (and the same for `column`)
- `` `scope` `name` is not a scalar column ``
- `` `#[db(relation)]` is only legal on a table row ``
- `` `#[db(relation)]` field must be `Option<T>` or `Vec<T>` ``
- `` `local_key` `name` is not a scalar column ``
- `` `local_key` and `target_key` must have the same length ``
- `` `#[db(unique)]` is only legal on a table row ``
- `` `#[db(unique)]` is redundant on a primary-key field ``
- `` `unique` column `name` is not a scalar column ``
- `` a `unique` key cannot repeat the primary key ``

All problems found in one struct are reported together, so a single compile
fixes the whole annotation block.

A non-relation field with neither `pgtype` nor a type that implements `DbType`
is not caught by the derive, because it cannot resolve types; rustc reports the
unsatisfied `dbkit::DbType` bound on the field type instead.

One check is not a compile error. `#[db(flatten)]` on a table or a subquery
type compiles, because all shapes are `dbkit::Model`; the generated `meta()`
matches `Meta` and panics on the other variants:

- `` `#[db(flatten)]` field `size` has type `<table>`, which is a table; flattened fields must be `#[db(fragment)]` structs ``
- `` `#[db(flatten)]` field `size` has type `<type>`, which is a subquery; flattened fields must be `#[db(fragment)]` structs ``

## Generated metadata

`#[derive(DbModel)]` emits `impl dbkit::Model for Email` with
`fn meta() -> dbkit::Meta`, whose `Meta::Table` variant carries a
`dbkit::TableMeta`:

- `table` — the value of `#[db(table = "...")]`.
- `primary_key` — the annotated key field names in declaration order.
- `version`, `created_at`, `updated_at`, `tenant` — the annotated field names,
  or `None` for absent optional roles. A table with `tenant` also implements
  `dbkit::TenantOwned` with `TENANT_COLUMN`.
- `columns` — every column in declaration order as
  `&'static dyn dbkit::ErasedColumn`, whose `name()`, `nullable()`,
  `pg_type()`, `pg_default()`, `generated()` and `query_ops()` answer like the
  generated `<Model>Column` variants. `nullable` is true exactly when the field
  type is `Option<_>`, `pg_type` is the field's `pgtype` annotation or its
  `DbType` type's `PG_TYPE`, `pg_default` is its `pgdefault` annotation, its
  `DbType` type's `PG_DEFAULT`, or `None`, `generated` is the `pg_virtual`
  expression or `None`, `query_ops` is the `query_ops` list in declaration order
  or an empty slice, `orderable` says whether the column declared
  `#[db(orderable)]`, and `triggers()` lists the trigger references that fill
  the column in declaration order, or an empty slice.
- `relations` — every `#[db(relation)]` field in declaration order, with its
  field name, model name, target table, cardinality, local key and target key.
- `foreign_keys` — every `#[db(foreign_key)]` in declaration order, with its
  local columns, target table and referenced columns.
- `unique_keys` — every field-level `#[db(unique)]` and struct-level
  `#[db(unique(columns = [...]))]`, with its column list.
- `triggers` — every `#[db(pg_trigger)]` declaration in declaration order as a
  `dbkit::TriggerRef { name, table }`. `table` is `on = "..."` or the model's
  table, so an entry may name another model's table.
- `trigger_definitions` — every declaration that carries a `body`, in
  declaration order, as a `dbkit::Trigger` with `name`, `table`, `timing`,
  `events`, `level`, `condition`, `function`, `language` and `body`. A name-only
  reference has no entry here.

A `#[db(subquery = "...")]` struct emits
`Meta::SubQuery(&SubQuery { sql, columns })` instead, where `sql` is the
annotation's string and `columns` is the same erased list. There is no table,
key or role to fill in.

A `#[db(fragment)]` struct emits `Meta::Fragment(&TableFragment { columns })`,
again with the same erased list and no table, key or role fields.

The list holds trait objects because a `#[db(flatten)]` field splices another
type's columns, so one table's description can mix columns from several models
and no single enum can name them all. The columns are the `<Model>Column`
variants the same expansion emits; the macro pushes each variant's reference
and extends the list from each flattened field's `Meta::Fragment` columns, then
leaks the slice into a `static OnceLock<TableMeta>`. The first call does the
allocation; every later call returns the same `Meta::Table`, pointing at the
cached `TableMeta`. A subquery uses a `OnceLock<SubQuery>` and a fragment a
`OnceLock<TableFragment>`, so every shape builds its columns one way.

`Meta` is an enum, so a fragment's value has no table name or key to lie about
and a subquery carries its SQL where a table carries its name.
`Meta::column(name)` looks a column up by name; `Meta::table()` and
`Meta::subquery()` narrow the shape. All shapes implement `Model`, so the type
system cannot tell them apart at the trait level; `flatten` matches the `Meta`
variant at runtime, and `Meta::into_table()` panics if a subquery or a fragment
is listed in `MODELS`.

The emitted code names `::dbkit::...`, so the deriving crate depends on
`dbkit` (which re-exports the derive).

## Generated keys

`#[derive(DbModel)]` on a table emits two key structs. `<Model>PK` holds exactly
the primary key fields:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPK {
    pub id: UserId,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for UserPK {
    // decodes by the primary key column names
}
```

`<Model>PKV` holds the same fields, then the tenant column when the model has
one that is not already part of the key, then the version column:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPKV {
    pub id: UserId,
    pub tenant_id: TenantId,
    pub version: i64,
}
```

- PK fields follow `TableMeta::primary_key`, which is model declaration order. A
  composite key contributes one field per key column.
- PKV appends `tenant_id` unless the primary key already contains it, then
  `version`. A model without a tenant or version gets an alias instead:
  `pub type UserPKV = UserPK;`.
- Both names have the model's visibility and work with `Debug`, `Clone`,
  `Serialize`, `Deserialize` and `sqlx::FromRow`, so a `RETURNING` list decodes
  into either.
- A fragment or a subquery has no primary key and gets neither struct.
- `DbInsert` uses PK as `InsertRow::Key`, so an insert returns the key rather
  than the whole row. `DbDelete` uses PKV as its input, so a delete names the
  row and checks its version.

## Generated patch

`#[derive(DbPatch)]` on a model emits a `<Model>Patch` struct next to it. A
subquery row cannot derive it: there is no table to update and no key to name
a row with.

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
- When the model has a `#[db(tenant)]` column, the patch also carries it as a
  required plain field and adds it to `WHERE`. It is not `Patch<T>`, so a patch
  cannot move a row to another tenant.
- Every other column becomes a `dbkit::Patch<T>` field: a missing key is
  `Patch::Absent` (leave the column alone), an explicit `null` is `Patch::Null`
  (clear a nullable column) and any other value is `Patch::Set`.
- A nullable column's field is `Patch<T>`, not `Patch<Option<T>>`, so `null`
  means "clear" rather than "set to null".
- `created_at` and `updated_at` are not patchable and are omitted.
- A `#[db(pg_virtual)]` field is omitted; PostgreSQL computes it. A
  trigger-maintained field, one with `#[db(pg_trigger)]`, is omitted too.
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
- The patch has a `new(...)` constructor taking the key columns in declaration
  order and then `version` when the model has one. Every business column starts
  as `Patch::Absent`. Each business column gets a `set_<field>(value)` method;
  a nullable column also gets `clear_<field>()`, which sets `Patch::Null`. A
  flattened field's setter takes the fragment's patch.

## Generated update

The same derive implements `dbkit::PatchRow` and, for a table, adds
`Model::patch(entries)`, which returns a `dbkit::PatchQuery`:

```rust
let patch = UserPatch::new(id, 3)
    .set_email(Email("new@example.com".to_owned()))
    .clear_email_verified_at();

User::patch([patch]).execute(&pool).await?;
```

- `PatchQuery::render()` returns one `dbkit::RenderedPatch` per entry; each is
  an `UPDATE` whose identifiers come from `TableMeta`.
- `Patch::Absent` columns are omitted, `Patch::Set` binds a value and
  `Patch::Null` writes SQL `NULL`.
- The primary key is the `WHERE` clause. A model with a version gets
  `"version" = $n` there and `"version" = "version" + 1` in `SET`; a model with
  `updated_at` gets `"updated_at" = now()`.
- `PatchQuery::execute` renders every entry before opening a transaction. A
  pool or plain connection starts one; a `&mut Transaction` creates a savepoint,
  so the caller can nest the batch.
- Every entry must update exactly one row. A missing row or a stale version is
  `PatchError::NotUpdated`, and the whole batch rolls back.
- An entry with no changed business column is `PatchError::Empty`; it would
  otherwise bump the version without changing anything.
- Patch payload types are exposed as `&dyn dbkit::Bind`, so a field type must
  implement `Bind` (`Type + Encode + Serialize + PartialEq`), like every query
  value.

See [Patch builder](patch-builder.md) for the transaction behavior, SQL shape
and error cases.

## Generated delete

`#[derive(DbDelete)]` on a table implements `dbkit::DeleteKey` for the
`{Model}PKV` that `DbModel` generates, and adds `Model::delete(entries)`:

```rust
User::delete([
    UserPKV { id: first, version: 3 },
    UserPKV { id: second, version: 7 },
])
.execute(&pool)
.await?;
```

- The input is the key struct, not a new `{Model}Delete` type. `DeleteKey::key`
  lists the primary key values in `TableMeta::primary_key` order, and
  `DeleteKey::version` is `Some` exactly when the model has a version.
- A tenant-owned model includes `tenant_id` in `PKV` and renders
  `"tenant_id" = $n` in `WHERE` unless the primary key already contains it.
- A versioned model renders `"version" = $n` in `WHERE`, so a stale version
  deletes nothing. A model without a version or tenant has
  `type {Model}PKV = {Model}PK` and deletes by key alone.
- `DeleteQuery::render()` returns one `dbkit::RenderedDelete` per entry, each a
  `DELETE` whose identifiers come from `TableMeta`.
- `DeleteQuery::execute` renders every entry before opening a transaction. A
  pool or plain connection starts one; a `&mut Transaction` creates a savepoint.
- Every entry must delete exactly one row. A missing row or a stale version is
  `DeleteError::NotDeleted`, and the whole batch rolls back, so a four-entry
  batch never leaves three rows deleted.
- A fragment or subquery model cannot derive `DbDelete`: there is no table to
  delete from.

See [Delete builder](delete-builder.md) for the transaction behavior, SQL shape
and error cases.

## Generated insert

`#[derive(DbInsert)]` emits `<Model>Insert`. Required columns are plain fields;
columns with a default and nullable columns are `Option` fields.

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
    pub version: Option<i64>,
}
```

- A required field is non-null and has no default. An optional field is omitted
  from the `INSERT` when it is `None`, so PostgreSQL applies the default or
  stores `NULL`.
- A `#[db(tenant)]` field is always required; the derive rejects a default, so
  the insert cannot leave the tenant to the database.
- A `#[db(pg_virtual)]` or trigger-maintained field is omitted; PostgreSQL
  computes it. A `#[db(relation)]` field is not a column and is omitted.
- `new(...)` takes the required fields in declaration order, plus each
  flattened fragment insert. Every optional field starts `None`.
- `set_<field>(value)` sets one column. There is no `clear_` method: on insert,
  “not given” means the database chooses, not SQL `NULL`.
- A flattened field nests the fragment's insert type with `serde(flatten)`.
- The struct derives `Serialize`, `Deserialize` and `garde::Validate`. Role
  fields (primary key, tenant, version, timestamps) are not validated; business
  fields whose payload type can carry rules are marked `#[garde(dive)]`.
- An inherited default (`pgdefault = inherit`) makes the field optional and, for
  a non-null column, adds a const assertion that the type has a default.

For a table, the derive also adds:

```rust
let insert = UserInsert::new(Email("new@example.com".to_owned()))
    .set_email_verified_at(Utc::now());

let inserted: Vec<UserPK> = User::insert([insert]).execute(&pool).await?;
```

- `InsertQuery::render()` returns one `RenderedInsert` per entry, each an
  `INSERT ... RETURNING <primary key>`.
- `InsertQuery::execute` renders every entry, runs them in one transaction and
  returns the inserted primary keys in input order. A `&mut Transaction`
  creates a savepoint; an empty batch is a no-op.
- An entry with every optional column `None` renders
  `INSERT INTO "table" DEFAULT VALUES RETURNING <primary key>`.
- The `RETURNING` list names the primary key columns, so the result decodes into
  `<Model>PK`. Other generated columns are read back by key.
- A supplied `created_at` also supplies `updated_at` when the latter is omitted,
  so imported rows keep their creation time.
- A constraint violation is `InsertError::Sqlx`; the whole batch rolls back.

See [Insert builder](insert-builder.md) for the SQL shape, builders and error
cases.

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
    pub fn generated(self) -> Option<&'static str> { /* the pg_virtual expression, ... */ }
    pub fn query_ops(self) -> &'static [FilterOperator] { /* ... */ }
    pub fn orderable(self) -> bool { /* ... */ }
    pub fn triggers(self) -> &'static [TriggerRef] { /* ... */ }
}

impl Serialize for WidgetColumn { /* serialize_str(self.name()) */ }
impl<'de> Deserialize<'de> for WidgetColumn { /* from_name() */ }

// one empty tag per column, one capability per query_ops entry and for
// `orderable`, and the typed handles the query builder takes
pub struct WidgetIdTag;
impl<C: Column> dbkit::HasEq<C> for WidgetIdTag {}
impl<C: Column> dbkit::HasOrder<C> for WidgetIdTag {}

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
WidgetColumn::Total.generated();           // Some("net_amount + tax_amount"), from #[db(pg_virtual)]
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
tree of filters into SQL, so a handler does not build the statement. With
`#[derive(DbSelect)]` on the model it also decodes the rows:

```rust
#[derive(DbModel, DbSelect)]
#[db(table = "widget")]
pub struct Widget {
    /* ... */
}

let query = Widget::query()
    .select(request.select.unwrap_or_else(Columns::all))
    .filters([Widget::columns().name.eq("example")]);
let rows: Vec<WidgetSelect> = query.build_selected()?.fetch_all(&pool).await?;
```

`render()` is the same rendering without running it, for a caller that wants the
SQL text and its parameters.

`sqlx::FromRow` cannot decode a partial projection: the derived implementation
reads every field by name. A sparse selection therefore only suits a response
built dynamically or a projection type that matches the requested columns.

`build_selected()` needs an explicit `select`; a query without one is
`Error::MissingSelection`, because the model is what decodes every column:

```rust
let rows: Vec<Widget> = Widget::query()
    .filters([Widget::columns().name.eq("example")])
    .build()?
    .fetch_all(&pool)
    .await?;
```

`build()` requires every column and rejects a `select` that leaves one out with
`Error::ProjectedModel`, before any SQL runs.

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
`TableMeta` (`TableMeta` is `Copy`), and panics for a subquery or a fragment.
A `LazyLock` keeps the list a single shared value instead of rebuilding it per
call. `&MODELS` coerces to `&[TableMeta]`, which is what the schema check and
the tests take. Tests iterate `MODELS.iter()`; omitting a model from the list
is caught by the schema test, which fails on any table without a model.
Subqueries and fragments are not tables, so they are not listed — and listing
one panics instead of silently checking a table named `""`. A schema test that covers a model with
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
`information_schema.columns`, tables without a model, types and defaults,
generated columns, foreign keys, unique indexes, triggers, and a column declared
more than once — which is what two `#[db(flatten)]` fragments sharing a column
name produce.
Types are compared structurally: the helper creates a temporary table whose
columns use the `pgtype` annotations and compares each column's `atttypid` and
`atttypmod` with the real table's, so length and precision must match and any
spelling PostgreSQL accepts works. Defaults and generated expressions are
compared from the same probe table: the helper adds each `pgdefault` as a
`DEFAULT` clause, or each `pg_virtual` as a `GENERATED ALWAYS AS (...) VIRTUAL`
clause, and compares the normalized expression PostgreSQL reports for it with
the real column's expression. A missing or extra default, generated kind or
expression is reported.

Triggers are collected from every model before they are compared, because a
field on one model may declare a trigger installed on another table. Names are
compared in both directions, so a live trigger without a declaration is a
problem too. For a declaration with a `body`, the check also compares timing,
events, row or statement level, function name, language and body; a `WHEN`
condition is parsed by PostgreSQL on a temporary trigger in `pg_temp` and the
two expression trees are compared, so formatting and identifier quoting do not
matter. A declaration without a `body` checks only the name.

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
- Deleting by an arbitrary filter: the query builder is read-only, and
  `DbDelete` deletes by primary key only.
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
- Loading a `#[db(relation)]`: the annotation records the field and the join,
  but dbkit does not execute the second query.
- Generating `schema.sql` from `MODELS`: triggers are fully described, but the
  generator does not exist yet. Non-unique indexes and foreign-key actions are
  not modelled either, so a generated schema would currently lose them. See
  [Schema SQL generator](todo-schema-sql-generator.md).
- Foreign-key `on delete` and `on update` actions: only the columns and target
  are compared.
- Stored generated columns: `#[db(pg_virtual)]` describes a virtual column.
- Propagating foreign keys, unique keys, triggers or relations through
  `#[db(flatten)]` fragments.
