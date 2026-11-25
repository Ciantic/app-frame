# Schema SQL generator (TODO)

Status: design only. `dbkit::schema::generate` does not exist yet. The models
already describe most of the DDL, so this document covers the missing metadata,
the generator's output, its integration and its tests.

The goal is to make `MODELS` the source of truth for a fresh database:

- One generated `schema.sql` per service, checked in and applied by the tests.
- No hand-maintained table, key, index or trigger DDL.
- The live schema check remains the final proof that generated SQL and models
  agree.

## What the models already describe

`TableMeta` and `ErasedColumn` provide:

- Table name and every column in declaration order, including flattened
  columns.
- Column type, nullability, default, generated expression and column name.
- Primary key, unique keys, foreign keys and their target columns.
- Trigger declarations and full definitions: name, target table, timing,
  events, level, condition, function, language and body.
- Roles that need no separate DDL: version, created-at, updated-at and tenant.
- Relations, query operators and orderability are runtime metadata and emit no
  DDL.

The generator does not need a database connection. It needs only
`&[TableMeta]`. It does not validate the SQL it emits; the round-trip test does
that by applying the result to a fresh database.

## Missing metadata

Two DDL objects in the current hand-written schema are not modelled yet.

### Non-unique indexes

Add a struct-level, repeatable annotation:

```rust
#[db(table = "invoice")]
#[db(index(columns = [customer_id]))]
#[db(index(columns = [tenant_id, created_at], name = "invoice_tenant_created_idx"))]
pub struct Invoice { /* ... */ }
```

- `columns` is a non-empty list of scalar columns, in index order.
- `name` is optional; the default is `<table>_<columns>_idx`, which matches
  PostgreSQL's own convention and the current index names.
- Only plain, non-unique, non-partial, non-expression indexes are modelled.
  `unique` already covers unique indexes.
- Metadata:

```rust
pub struct Index {
    pub name: &'static str,
    pub columns: &'static [&'static str],
}
```

- `TableMeta` gains `pub indexes: &'static [Index]`.
- The derive checks that each column is scalar and that no index repeats a
  column or another index.
- The schema check compares non-unique, non-primary indexes from `pg_index` in
  both directions, by name and columns. Partial and expression indexes stay
  ignored.

### Foreign-key actions

Extend `foreign_key`:

```rust
#[db(foreign_key(
    table = "invoice",
    column = "id",
    scope = "tenant_id",
    on_delete = cascade
))]
pub invoice_id: InvoiceId,
```

- Values: `no_action`, `restrict`, `cascade`, `set_null`, `set_default`.
- `on_update` takes the same values. Both default to `no_action`.
- Metadata:

```rust
pub enum ForeignKeyAction {
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

pub struct ForeignKey {
    pub columns: &'static [&'static str],
    pub table: &'static str,
    pub referenced_columns: &'static [&'static str],
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
}
```

- The schema check adds `confdeltype` and `confupdtype` from `pg_constraint` to
  the comparison. The existing `invoice_row` foreign key needs
  `on_delete = cascade`.

### What remains unsupported

These are not generated and must be created outside the generated file, or
added as annotations when a model needs them:

- Check and exclusion constraints.
- Deferrable or initially deferred constraints.
- Partial, expression, covering and concurrent indexes.
- Stored generated columns; `pg_virtual` is virtual only.
- Domains, enums, composite types, extensions, sequences, views, materialized
  views and functions that are not trigger functions.
- Grants, row-level security, policies, comments, collations, partitioning and
  table storage parameters.

The generated file is not a migration. It applies to an empty database.

## Service composition

The deployment is one database with one PostgreSQL schema. Services are Rust
crates that own models, not separate database schemas. A backend crate depends
on the services, combines their `MODELS`, and is the composition root for the
database.

### Current composition

`dbkit::schema::Schema` is the interim mechanism, before the generator exists:

```rust
pub struct Schema {
    pub name: &'static str,
    pub sql: &'static str,
    pub models: &'static LazyLock<Vec<TableMeta>>,
    pub dependencies: &'static [&'static Schema],
}
```

Each service exposes a `SCHEMA` static. `invoice-service` declares its
dependency on `user-service` at that level:

```rust
// crates/invoice-service/src/db/schema.rs
pub static SCHEMA: Schema = Schema {
    name: "invoice",
    sql: include_str!("../db/schema.sql"),
    models: &MODELS,
    dependencies: &[&user_service::db::schema::SCHEMA],
};
```

- `SCHEMA.all_sql()` returns dependency SQL first, then the service's own SQL,
  with each service included once.
- `SCHEMA.all_models()` does the same for `TableMeta`.
- `SCHEMA.apply(&db)` applies the composed SQL to a pool.
- `SCHEMA.check(&mut connection)` runs
  `check_live_schema_against_models` against the composed model list.

An invoice test therefore needs no knowledge of `user-service`:

```rust
SCHEMA.apply(&db).await.unwrap();
let mut connection = db.acquire().await.unwrap();
let problems = SCHEMA.check(&mut connection).await.unwrap();
```

A compile-time `concat!` cannot do this: it accepts literals only, and
`user_service::db::schema::SCHEMA` is a cross-crate `const`, not a literal.
Including the other crate's file by a relative `include_str!` path would be
fragile and would duplicate the SQL. The `Schema` value carries the link and
the models instead.

`SCHEMA` is a service-level composition. It is still fine for a single service
to test its own tables, because the composed SQL and models include the
dependencies.

### Future composition

Once the generator exists, the backend can build one combined `MODELS` and
generate one schema. The backend owns the combined descriptors:

```rust
pub static MODELS: LazyLock<Vec<TableMeta>> = LazyLock::new(|| {
    let services: &[&[TableMeta]] = &[
        &user_service::db::models::MODELS[..],
        &invoice_service::db::models::MODELS[..],
        &email_service::db::models::MODELS[..],
    ];

    services
        .iter()
        .flat_map(|models| models.iter().copied())
        .collect()
});

pub static SCHEMA: Schema = Schema {
    name: "backend",
    sql: include_str!("../db/schema.sql"),
    models: &MODELS,
    dependencies: &[],
};
```

The checked-in `schema.sql` is produced by `dbkit::schema::generate(&MODELS)`; the
backend does not generate it at runtime.

- Each service keeps its own `MODELS`; that is what the backend composes.
- A service-local `schema.sql` is a fragment for tests, not the deployment
  artifact. The backend's generated schema is the deployment artifact.
- Table names must be globally unique across services. The generator reports a
  duplicate table name before emitting SQL.
- Cross-service foreign keys are ordinary foreign keys. No `external` marker is
  needed because the target table is in the combined `MODELS`.
- The generator emits every table first and every foreign key with
  `alter table` afterwards, so service order and foreign-key cycles do not
  matter.
- Triggers are still local in ownership even though they share the schema. An
  invoice trigger may write invoice tables; do not write another service's
  tables from it.

For the invoice tenant:

```rust
#[db(tenant)]
#[db(foreign_key(table = "tenant", column = "id"))]
pub tenant_id: TenantId,
```

- `tenant` is `user-service`'s table but is present in the combined `MODELS`,
  so the schema test and generator treat it as a normal foreign key.
- Add the same foreign key to `Customer.tenant_id` and, if desired,
  `InvoiceRow.tenant_id`. `InvoiceRow` is already constrained transitively by
  its `(tenant_id, invoice_id)` reference to `Invoice`; a direct tenant foreign
  key is optional but makes the tenant invariant explicit.
- `seller_id` stays opaque until a seller service and table join the
  composition.
- Decide `on delete` for the tenant foreign keys. `cascade` deletes every
  service's tenant rows when the tenant is deleted; `restrict` or the default
  `no action` forces an explicit tenant-deletion workflow. The choice is a
  product decision, not a generator detail.

Service tests use the service's `SCHEMA` rather than composing dependencies by
hand:

- `SCHEMA.apply` applies the dependency fragments first, so a service-local
  `schema.sql` may keep inline cross-service foreign keys.
- `SCHEMA.check` passes the combined `TableMeta` list, so the checker does not
  report the dependency's tables as unmodelled.
- Insert the referenced rows. Invoice tests that use a tenant id need a
  `tenant` row before the customer, invoice or row insert, or the foreign key
  rejects it.
- Keep a separate backend integration test that applies the full combined
  schema and checks the full combined model list. That is the authoritative
  cross-service test.

A service-local `schema.sql` with inline cross-service foreign keys only works
when its dependencies are applied first; `SCHEMA.all_sql` guarantees that
order. Once the generator exists, prefer the generated combined schema and
`alter table` for foreign keys. Concatenating per-service generated files is
not a composition strategy: each file may contain `alter table` statements
whose target table is in a later file.

`TenantId` currently lives in `user-service`, and `invoice-service` imports it
from there. That is a Rust type dependency, not a database dependency, but it
still couples the build. A small shared contract crate for ids such as
`TenantId` and `UserId` keeps the service boundary explicit without making one
service depend on another. It does not change the single-schema composition.

## Generator API

Add to `crates/dbkit/src/schema.rs`:

```rust
/// Renders a fresh schema for `models`; one problem per model inconsistency.
pub fn generate(models: &[TableMeta]) -> Result<String, Vec<String>>;

/// The same statements, for tests and tools that inspect them individually.
pub fn statements(models: &[TableMeta]) -> Result<Vec<String>, Vec<String>>;
```

- `generate` joins statements with a blank line and a trailing newline.
- `statements` is the useful unit for tests: one `create table`, one
  `alter table`, one index, one function and one trigger per element.
- Errors use the same shape and tone as
  `check_live_schema_against_models`: one line per problem, no panic. Most
  problems are cross-model: conflicting trigger definitions, duplicate
  function names, a trigger on an unmodelled table, duplicate tables,
  duplicate index names and generated identifiers over 63 bytes.
- A checked-in generated file is preferred to runtime generation. It is
  reviewable, it can be diffed, and it keeps the SQL a `&'static str` through
  `include_str!` inside `Schema`.

## Output layout

The output is lowercase DDL with quoted identifiers, matching the current
`schema.sql` files.

Order:

1. Every table, with its columns, primary key and unique constraints inline.
   These have no cross-table dependencies.
2. Foreign keys as `alter table ... add constraint ...`, after all tables. This
   keeps table order irrelevant and handles cycles.
3. Non-unique indexes.
4. Trigger functions, deduplicated by function name and sorted by name.
5. Triggers, sorted by table and name.

Do not emit `begin` or `commit`; the caller decides whether to wrap the schema
in a transaction. Do not emit `if not exists` or drops; the file targets a
fresh database. A short header comment is useful:

```sql
-- Generated from dbkit models; do not edit. Regenerate with `cargo run -p invoice-service --bin schema`.
```

### Table example

For an `invoice` table the generator would emit roughly:

```sql
create table "invoice" (
    "id" uuid not null default uuidv7(),
    "tenant_id" uuid not null,
    "number" text,
    "customer_id" uuid,
    "net_amount" numeric(14,2) not null default 0,
    "tax_amount" numeric(14,2) not null default 0,
    "total_amount" numeric(14,2) generated always as ("net_amount" + "tax_amount") virtual not null,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "version" bigint not null default 0,
    constraint "invoice_pkey" primary key ("id"),
    constraint "invoice_number_key" unique ("number"),
    constraint "invoice_tenant_id_id_key" unique ("tenant_id", "id")
);

alter table "invoice"
add constraint "invoice_customer_id_fkey"
foreign key ("tenant_id", "customer_id") references "customer" ("tenant_id", "id");

create index "invoice_customer_id_idx" on "invoice" ("customer_id");
```

Column rules:

- Emit `"<name>" <pg_type>` from the metadata.
- Append `not null` when `nullable` is false.
- Append `default <pg_default>` when the column has a default.
- Append `generated always as (<expression>) virtual` when the column is
  generated, followed by `not null` when applicable. `pg_virtual` and
  `pgdefault` cannot combine, so the two branches stay exclusive.
- Emit the type, default and generated expression verbatim. They are already
  DDL and may contain commas, casts and array brackets.
- Preserve column order, including columns spliced in by `#[db(flatten)]`.

Constraint rules:

- Primary key: `<table>_pkey`.
- Unique key: `<table>_<column>_<column>_key`, in the metadata's column order.
- Foreign key: `<table>_<column>_<column>_fkey`.
- Non-unique index: the annotation's `name` or
  `<table>_<column>_<column>_idx`.
- Quote and escape every identifier. PostgreSQL truncates identifiers over 63
  bytes; the generator should report a problem instead of emitting a name it
  would silently truncate. A future improvement may hash long names, but an
  explicit name in the annotation is the clearer fix.

Foreign-key actions:

- Omit the action when it is `no_action`.
- Otherwise emit `on delete <action>` and `on update <action>` from the
  metadata.
- A foreign key may reference a table outside `MODELS`; the generated file does
  not create that table, and applying the schema requires it to exist. This is
  allowed, because external ids such as a seller or tenant have no local table.
- A column default or generated expression may also reference functions,
  domains or extensions that the generated file does not create. Those must
  exist before the schema is applied.

### Trigger example

```sql
create function "invoice_totals"() returns trigger language plpgsql as $$
begin
    new."net_amount" := coalesce((
        select sum("net_amount") from "invoice_row"
        where "invoice_id" = new."id" and "tenant_id" = new."tenant_id"
    ), 0);
    return new;
end;
$$;

create trigger "invoice_totals"
before insert or update on "invoice"
for each row
execute function "invoice_totals"();
```

Trigger rules:

- Collect trigger definitions from every model into a global map keyed by
  `(table, name)`. A field on one model may define a trigger installed on
  another table, so per-model rendering would duplicate or miss it.
- Identical duplicate definitions collapse. Differing definitions for the same
  key are a problem.
- A defined trigger whose `table` is not in `MODELS` is a problem. The
  generated file cannot create a trigger on a table it does not own.
- Group functions by name. Different bodies or languages under one function
  name are a problem.
- The default function name is the trigger name. Two triggers on different
  tables may therefore collide; the error names both triggers and points at
  `function = "..."` as the fix.
- Emit events in canonical order: insert, update, delete, truncate.
- Emit `for each row` or `for each statement` from `level`.
- Emit `when (<condition>)` when the definition has a condition.
- Emit the function body with a dollar-quote tag that does not occur in the
  body. Try `$$`, then `$dbkit$`, `$dbkit_1$`, and so on.
- Quote function, trigger and table names. A function name is one plain
  identifier; schema-qualified names are out of scope.

## Integration

Add one generator binary to the backend composition crate:

```rust
// crates/backend/src/bin/schema.rs
use backend::db::schema::MODELS;

fn main() {
    match dbkit::schema::generate(&MODELS) {
        Ok(sql) => print!("{sql}"),
        Err(problems) => {
            for problem in problems {
                eprintln!("{problem}");
            }
            std::process::exit(1);
        }
    }
}
```

Regenerate with:

```bash
cargo run -p backend --bin schema > crates/backend/src/db/schema.sql
```

The backend keeps its generated file as `include_str!`. Add a drift test:

```rust
#[test]
fn schema_sql_is_generated() {
    let generated = dbkit::schema::generate(&MODELS).unwrap();
    assert_eq!(
        generated,
        SCHEMA.sql,
        "regenerate with `cargo run -p backend --bin schema`"
    );
}
```

The existing live test still applies `SCHEMA` and runs
`check_live_schema_against_models` with the combined `MODELS`, so a generated
file that omits an object fails there too.

Per-service binaries are optional and only useful for local test fragments.
They must not become the deployment artifact: a service-local file cannot see
another service's tables, and concatenating generated files is not safe.

Runtime generation, `LazyLock<String>`, is possible but not recommended first:
it removes the reviewable file and makes the schema invisible in a diff.

## Testing

Add, in order:

1. Unit tests for each statement shape: column types, nullability, defaults,
   generated columns, primary keys, unique keys, foreign keys, actions,
   indexes, trigger functions, trigger conditions and dollar-quote selection.
2. A round-trip test: apply generated SQL to a fresh `PgtestServer` database
   and expect `check_live_schema_against_models` to return no problems. This
   catches missing DDL even before the hand-written file is replaced.
3. An error test for each cross-model problem: conflicting trigger
   definitions, conflicting function bodies, a trigger on a table without a
   model, duplicate table names and a generated identifier over 63 bytes.
4. A drift test once the backend schema is generated: `generate(&MODELS)`
   equals the checked-in combined file.
5. A determinism test: generating twice returns the same bytes. Tables follow
   `MODELS` order; functions and triggers are sorted, so the output does not
   depend on hash-map iteration.

## Migration plan

1. Add the `index` annotation and `TableMeta.indexes`, and extend the schema
   check.
2. Add `on_delete` and `on_update` to `foreign_key`, update the metadata and
   the schema check.
3. Implement `generate` and `statements` with unit tests.
4. Add the backend composition crate, its combined `MODELS` and the `schema`
   binary, but keep the hand-written fragments for one commit.
5. Add the round-trip test and fix every difference it reports. This is the
   point where missing indexes or foreign-key actions surface.
6. Replace the deployment schema with the backend's generated file and add the
   drift test. Keep per-service fragments only for isolated tests, or remove
   them once those tests apply the composed schema.
7. Only then consider generating at runtime.

## Open decisions

- Explicit constraint and index names versus letting PostgreSQL name them.
  This document recommends explicit names for stable diffs.
- Table order: `MODELS` order versus alphabetical. This document recommends
  `MODELS` order for readability; foreign keys are added later, so order is not
  a correctness requirement.
- Function-name collisions: error with a pointer to `function = "..."` versus
  automatically prefixing the table name. This document recommends the error,
  because an automatic name change is invisible in the annotation.
- Trigger target tables outside `MODELS`: reject versus allow when the table is
  owned by another schema. This document recommends rejecting for the first
  version, so one generated file describes one complete schema.
- Whether the backend's `schema.sql` remains checked in and drift-tested, or
  `SCHEMA` is generated at runtime. This document recommends the checked-in
  file.
