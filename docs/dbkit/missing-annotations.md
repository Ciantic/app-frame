# Missing dbkit annotations

Converting the invoice models exposed five annotations dbkit still lacks:

- `relation`
- `pg_trigger`
- `foreign_key`
- `pg_virtual`
- `unique`

This document proposes how to add them. It is a design record, not a
description of implemented behaviour. Once an annotation lands,
[annotations.md](annotations.md) remains the authoritative reference.

## Extension points

The current implementation has five places to touch for each new annotation:

- `crates/dbkit-macros/src/annotations.rs` parses and validates the vocabulary.
- `crates/dbkit-macros/src/derive_insert.rs` and `derive_patch.rs` decide which
  columns become input fields.
- `crates/dbkit-macros/src/lib.rs` emits the model metadata.
- `crates/dbkit/src/model.rs` holds `TableMeta`, `Column` and `ErasedColumn`.
- `crates/dbkit/src/schema.rs` compares metadata with the live database.

The derives read the macro-side `annotations::Column`, while runtime consumers
read `dbkit::Column` and `TableMeta`. An annotation that affects writes must be
handled in both places, even when only one of them needs the value.

## Design rules

- `schema.sql` stays the source of truth for DDL. Annotations record what
  builders and the schema test need; they do not generate tables today.
- New annotations are optional, so existing models keep compiling.
- Field-level annotations describe columns or related fields. Table-level
  annotations describe constraints and triggers, because those can span several
  columns.
- Unknown or repeated annotations remain compile errors.
- Metadata is built once in a `OnceLock`, so slices are leaked the same way the
  column list is leaked today.
- Validation that needs another type or the live database stays in the schema
  test; the macro only validates what it can see in the current struct.

## `relation`

`@relation` names a field that carries another model, not a column. The field
needs two things beyond its type: whether it holds one row or many, and the join
that finds the related rows. Do not infer the join from the field name.

### Syntax

One related row:

```rust
#[db(relation(
    one,
    table = "customer",
    local_key = [tenant_id, customer_id],
    target_key = [tenant_id, id]
))]
#[sqlx(skip)]
#[serde(skip_serializing_if = "Option::is_none")]
pub customer: Option<Customer>,
```

Many related rows:

```rust
#[db(relation(
    many,
    table = "invoice_row",
    local_key = [tenant_id, id],
    target_key = [tenant_id, invoice_id]
))]
#[sqlx(skip)]
#[serde(skip_serializing_if = "Vec::is_empty")]
pub rows: Vec<InvoiceRow>,
```

A single-column key can be written without brackets:

```rust
#[db(relation(one, table = "customer", local_key = customer_id, target_key = id))]
pub customer: Option<Customer>,
```

The cardinality word is explicit even though the field type also carries it.
`one` means one related row, `many` means a list. `T` is the related model and
its last path segment is recorded as `model`.

### Semantics

- `one` is a to-one cardinality, not a one-to-one uniqueness constraint; the
  related table may have many rows pointing at this one.
- `one` pairs with `Option<T>` in this pass. A required to-one field is not
  supported because the row query cannot fill it.
- `many` pairs with `Vec<T>`.
- `table` is the related table. `local_key` names one or more columns of this
  table; `target_key` names the same number of columns of the related table, in
  the same order. The join is `local_key = target_key`.
- The keys are not necessarily a declared foreign key. A relation may join on
  any columns; `foreign_key` separately describes the constraint.
- A composite key is how a relation stays inside one tenant:
  `[tenant_id, customer_id]` joins to `[tenant_id, id]` and
  `[tenant_id, id]` joins to `[tenant_id, invoice_id]`.
- A relation is not a column: `#[derive(DbModel)]` leaves it out of
  `TableMeta.columns`, `#[derive(DbInsert)]` and `#[derive(DbPatch)]` leave it
  out of their inputs, and `#[derive(DbSelect)]` leaves it out of the projection
  type.
- The field still needs `#[sqlx(skip)]`; dbkit does not rewrite the struct, and
  `sqlx::FromRow` otherwise tries to read the field as a column.
- `relation` legally combines only with serde and sqlx attributes. It must not
  combine with `pgtype`, `pgdefault`, `query_ops`, `orderable`, a role,
  `flatten`, `unique`, `foreign_key` or `pg_virtual`.
- It is legal on a table row only; a fragment or a subquery cannot carry it.
- dbkit does not load relations. A higher layer fills the field after the row
  query, or builds a read model. The annotation records the field and the join;
  it does not execute the join.
- `foreign_key` and `relation` are independent: `customer_id` describes the
  constraint, `customer` describes the serialized related object. A model can
  have either, both or neither.

### Metadata

Record the field, cardinality and join so an API or loader layer can fill it:

```rust
pub enum Cardinality {
    One,
    Many,
}

pub struct Relation {
    pub field: &'static str,
    pub model: &'static str,
    pub table: &'static str,
    pub cardinality: Cardinality,
    pub local_key: &'static [&'static str],
    pub target_key: &'static [&'static str],
}

pub struct TableMeta {
    // Existing fields stay as they are.
    pub relations: &'static [Relation],
}
```

`model` is the last path segment of the field type's inner type (`Customer` for
`Option<Customer>`, `InvoiceRow` for `Vec<InvoiceRow>`). It is a name for
discovery, not a type reference. `local_key` and `target_key` are stored in the
order the annotation lists them.

### Parser and validation

- Add `relation` to the field annotations and a separate `Annotations.relations`
  list, not to `annotations.columns`.
- Parse `relation(one, ...)` or `relation(many, ...)`; reject a missing or
  repeated cardinality.
- Require `table` as a non-empty string literal.
- Require non-empty `local_key` and `target_key` lists; both lists must be the
  same length. Accept a single bare identifier or a bracketed list.
- Check every `local_key` against the model's scalar columns; reject an unknown
  name and a flattened column.
- Require `Option<T>` for `one` and `Vec<T>` for `many`; reject the mismatch.
- Reject `relation` combined with any column annotation or role.
- Reject `relation` on a fragment or subquery.
- Reject a repeated `relation` on one field.

The macro cannot see the target table, so it cannot check `target_key` against
the related model.

### Schema test

No required check. A relation is not a DDL object, and the related table may be
owned by another service.

A later best-effort check can validate the names when the target table exists
in the current schema:

- `local_key` is already checked against the model at compile time;
- `target_key` can be checked against `information_schema.columns`;
- a missing target table is skipped rather than reported, so cross-service
  relations remain legal.

### Tests and invoice example

- Macro tests: one and many, single and composite keys, a missing table, a
  missing key, unequal key lengths, an unknown local key, a mismatched field
  type, a combined column annotation, a fragment and a subquery.
- JSON tests: an unset relation is omitted; a set relation serializes.
- The invoice models can add the two fields above without changing the table
  row query; both are filled by the caller and skipped by `FromRow`.
- `@children` maps to `many`; no separate annotation is needed.

## `pg_trigger`

A trigger is table behaviour, not a column property. The source attaches the
trigger to a field, but its `on InvoiceRow` clause gives the real target away:
the trigger lives on `invoice_row` and updates `invoice`. The model should say
that on the table that owns the trigger.

### What the annotation should mean

Do not copy the trigger body into the Rust model. `schema.sql` already holds
the function and the trigger, and dbkit does not generate DDL. Duplicating the
body would create two sources of truth and invite drift that the schema test
cannot reliably compare.

Use a name marker instead:

```rust
#[db(table = "invoice")]
#[db(pg_trigger = "invoice_totals")]
pub struct Invoice { /* ... */ }

#[db(table = "invoice_row")]
#[db(pg_trigger = "invoice_row_amounts")]
#[db(pg_trigger = "invoice_row_totals")]
pub struct InvoiceRow { /* ... */ }
```

The name is the stable contract. The trigger's timing, events, function and
body stay in `schema.sql`. The schema test catches a renamed or missing
trigger; it does not pretend to compare trigger bodies.

If dbkit gains schema generation later, extend the annotation with timing,
events and a body. That is a separate project and should not be guessed at now.

### Metadata

```rust
pub struct TableMeta {
    // Existing fields stay as they are.
    pub triggers: &'static [&'static str],
}
```

Trigger names are table-local, so a plain list of strings is enough. The macro
leaks the slice like the column list.

### Parser and validation

- Accept `pg_trigger = "name"` on the struct and repeat it for several triggers.
- Reject a non-string, empty or duplicate name.
- Reject `pg_trigger` on a fragment or subquery.
- Keep the annotation spelling `pg_trigger` to match `pgtype` and `pgdefault`.

### Schema test

Query non-internal trigger names for the model's table:

```sql
SELECT trigger.tgname
FROM pg_trigger AS trigger
WHERE trigger.tgrelid = to_regclass($1)
  AND NOT trigger.tgisinternal
```

Compare both directions: every declared trigger exists, and every non-internal
trigger is declared. Filtering `tgisinternal` is essential because PostgreSQL
implements foreign keys with internal constraint triggers.

The existing invoice schema becomes:

- `invoice`: `invoice_totals`;
- `invoice_row`: `invoice_row_amounts` and `invoice_row_totals`.

### Tests and invoice example

- Macro tests: one trigger, several triggers, empty name and duplicate name.
- Live tests: matching triggers, one missing, one extra, and an internal
  foreign-key trigger that must not be reported.
- Migrate the invoice models to the annotation and keep the trigger DDL in
  `schema.sql`.

`pg_trigger` does not mark a column read-only by itself. A trigger-maintained
column stays an ordinary input field in this pass; the database trigger
overwrites it. Excluding such fields from insert and patch is a separate
question.

## `foreign_key`

A foreign key is metadata rather than query-builder input. PostgreSQL already
enforces it at runtime, so the value of the annotation is discoverability and
schema drift detection. It also gives `scope = tenant_id` a home, which is the
part of the source annotation that keeps references inside one tenant.

### Syntax

Single-column foreign key:

```rust
#[db(foreign_key(table = "customer", column = "id"))]
pub customer_id: Option<CustomerId>,
```

Tenant-scoped foreign key:

```rust
#[db(foreign_key(table = "customer", column = "id", scope = "tenant_id"))]
pub customer_id: Option<CustomerId>,
```

The source form omits the target column because it resolves the target entity's
primary key. dbkit names it explicitly, so the macro does not need another
model's metadata and the annotation stays a plain table/column declaration.

The scope form expands to the local columns `("tenant_id", "customer_id")` and
the referenced columns `("tenant_id", "id")`. This is the shape the invoice
service needs for both `invoice.customer_id` and `invoice_row.invoice_id`:

```rust
#[db(foreign_key(table = "customer", column = "id", scope = "tenant_id"))]
pub customer_id: Option<CustomerId>,

#[db(foreign_key(table = "invoice", column = "id", scope = "tenant_id"))]
pub invoice_id: InvoiceId,
```

A fully general composite foreign key does not fit a single field. If that is
needed, add a struct-level form later:

```rust
#[db(foreign_key(
    columns = [tenant_id, invoice_id],
    table = "invoice",
    referenced_columns = [tenant_id, id]
))]
```

### Semantics

- `table` and `column` are required, non-empty string literals.
- `scope` is optional and must name another scalar column in the same table.
  The annotated field must not be the scope column.
- Without `scope`, the local and referenced column lists are one column each.
- With `scope`, the local and referenced columns both start with the scope
  column. The phase-one rule is that the target uses the same scope column
  name; a different name needs a later `referenced_scope` option.
- The annotation is legal on a scalar field, including an `Option` field. It is
  not legal on `#[db(flatten)]`, a fragment or a subquery.
- At most one foreign key per field.
- The macro cannot check the target column type or uniqueness. PostgreSQL
  enforces both, and the schema test only checks that the constraint matches
  the metadata.
- The field stays an ordinary field: keys, inserts, patches, query filters and
  JSON are unchanged.

### Metadata

Add a table-level list, because a foreign key can span columns:

```rust
pub struct ForeignKey {
    pub columns: &'static [&'static str],
    pub table: &'static str,
    pub referenced_columns: &'static [&'static str],
}

pub struct TableMeta {
    // Existing fields stay as they are.
    pub foreign_keys: &'static [ForeignKey],
}
```

The macro resolves the scope after it has seen every field, then leaks the
foreign-key slice next to the column slice. A foreign key inside a fragment
would have to be spliced through `TableFragment`; defer that until a fragment
actually needs one.

### Parser and validation

- Add `foreign_key` to `FieldAnnotations`; resolve it into
  `Annotations.foreign_keys` after parsing.
- Reject a repeated `foreign_key` on one field.
- Reject an empty `table` or `column`.
- Reject `scope` when the named field is missing, flattened, or the annotated
  field itself.
- Reject `foreign_key` combined with `flatten`.
- Reject `foreign_key` on a fragment or subquery.
- Keep the error vocabulary camel-free: `foreign_key`, not `foreignKey`.

### Schema test

Query `pg_constraint` for `contype = 'f'` on the model's table and compare:

- local columns, resolved from `conkey` through `pg_attribute`, in order;
- referenced table, resolved from `confrelid` through `pg_class`;
- referenced columns, resolved from `confkey` through `pg_attribute`, in order.

The shape is the same check the column test already does:

- a model foreign key with no constraint is a problem;
- a constraint with no model foreign key is a problem;
- a column-list or target mismatch is a problem.

`on delete` and `on update` actions are not recorded in phase one. Add them as
an enum later if the service needs the test to pin them down.

### Tests and invoice example

- Macro tests: one-column, scoped, missing scope, duplicate, empty names,
  flattened field, fragment and subquery.
- Live tests: matching constraint, missing constraint, extra constraint and a
  swapped column order.
- Migrate `invoice.customer_id` and `invoice_row.invoice_id` in
  `invoice-service`.

## `pg_virtual`

A generated column is computed by PostgreSQL from an expression and cannot be
written by `INSERT` or `UPDATE`. dbkit needs to know that so the field does not
appear in the insert and patch inputs.

### Syntax

```rust
#[db(pg_virtual = "\"net_amount\" + \"tax_amount\"")]
pub total_amount: Money,
```

The expression is DDL, written with the database's quoted identifiers.

### Semantics

- `pg_virtual` implies read-only: `#[derive(DbInsert)]` and `#[derive(DbPatch)]`
  leave the field out of their types and builders.
- The field stays in `TableMeta.columns`, `#[derive(DbColumns)]` and
  `#[derive(DbSelect)]`; reads and filters are unchanged.
- `pg_virtual` is legal on a scalar field, including an `Option` field. It is
  not legal on `#[db(flatten)]`, a fragment or a subquery.
- It must not combine with a role. A generated primary key or version is a
  different feature and needs an explicit decision.
- It may combine with `query_ops` and `orderable`, because the column is
  readable.
- The source's `@pgVirtual` uses camelCase column names; the annotation uses
  the real DDL names from `schema.sql`.

### Metadata

Expose the expression on the column; a present expression is also the
read-only flag:

```rust
pub trait Column {
    // Existing methods stay as they are.
    fn generated(self) -> Option<&'static str>;
}
```

`ErasedColumn` forwards the same method. `generated` carries the expression for
the schema test and for documentation, and `Some` tells generic layers that the
column is not writable.

### Insert and patch behavior

- Skip a generated column in `derive_insert` and `derive_patch` before the
  generic column handling.
- Keep it in `derive_columns`, `derive_select` and `TableMeta.columns`.
- `DbDelete` needs no change; it uses the key only.

### Schema test

- Check `pg_attribute.attgenerated` to confirm the column is generated and to
  distinguish stored from virtual.
- Recreate the expression on a probe column and compare PostgreSQL's normalized
  `pg_get_expr` output with the real column, the same way `pgdefault` is
  checked.

The invoice service currently computes every amount with triggers, so
`total_amount` is the natural first `pg_virtual` candidate; `net_amount` and
`tax_amount` depend on other rows and stay trigger-maintained.

### Tests and invoice example

- Macro tests: the annotation parses, repeats are rejected, and combining it
  with a role or `flatten` is rejected.
- Derive tests: the generated field is absent from insert and patch inputs.
- Live tests: a matching expression, a different expression and a non-generated
  column.
- Add `pg_virtual` to `invoice.total_amount` after the schema test can check it.

## `unique`

`@unique` is a constraint, not a claim that the field is a query key. The
schema test currently ignores unique constraints entirely.

### Syntax

Single-column:

```rust
#[db(unique)]
pub number: Option<String>,
```

Composite, for the `(tenant_id, id)` keys that back scoped foreign keys:

```rust
#[db(table = "invoice")]
#[db(unique(columns = [tenant_id, id]))]
pub struct Invoice { /* ... */ }
```

### Semantics

- `unique` on a field contributes a one-column key.
- The struct-level form contributes a key of its own.
- Primary keys are already unique and are not repeated here.
- A unique key does not imply `NOT NULL`; a nullable unique column keeps
  PostgreSQL's rule that multiple `NULL`s are allowed.
- `unique` does not change query builders or insert/patch inputs.

### Metadata

```rust
pub struct UniqueKey {
    pub columns: &'static [&'static str],
}

pub struct TableMeta {
    // Existing fields stay as they are.
    pub unique_keys: &'static [UniqueKey],
}
```

### Parser and validation

- Reject a repeated `unique` on one field.
- Reject an empty column list in the struct-level form.
- Reject unknown or non-identifier columns in the list.
- Reject `unique` combined with `flatten`; a flattened column's uniqueness
  would need fragment propagation.
- Reject `unique` on a fragment or subquery.
- Reject `unique` on a primary-key field; the key already enforces it.

### Schema test

Query `pg_index` for unique, non-primary indexes and compare their key column
sets with `TableMeta.unique_keys`. `pg_index` catches both constraints created
with `UNIQUE` and plain unique indexes, which `pg_constraint` alone does not.
Exclude partial and expression indexes unless a later annotation models them.

Check both directions, like columns:

- a declared unique key with no index is a problem;
- a unique index with no declared key is a problem.

This makes `invoice_number_key`, `invoice_tenant_id_id_key` and
`customer_tenant_id_id_key` explicit in the models.

### Tests and invoice example

- Macro tests: single, composite, repeated, empty list and unknown column.
- Live tests: matching index, missing index and extra index.
- Annotate `invoice.number` and the two `(tenant_id, id)` keys.

## Implementation order

1. `relation`: smallest change, affects only model shape and serialization.
2. `pg_trigger`: table metadata plus the non-internal trigger check.
3. `foreign_key`: metadata plus the `pg_constraint` check.
4. `pg_virtual`: write exclusion plus generated-column metadata.
5. `unique`: field and composite keys plus the `pg_index` check.

`pg_virtual` is the only one that changes write inputs, so it should land with
derive tests before the invoice model starts using it.

## Compatibility

- New annotations are additive; existing models compile and behave as before.
- Adding required methods to `Column` and `ErasedColumn` is a breaking change
  for hand-written implementations. The repository has none, and this project
  is explicitly not production, so the cleaner trait is acceptable.
- New `TableMeta` fields are construction-breaking for code that builds the
  struct by hand. `TableMeta` is produced by the derive; tests use
  `Model::meta()`, so the fallout is limited.
- A relation field needs `#[sqlx(skip)]`; document that next to the annotation
  so a model does not fail to decode at runtime.
- Update `annotations.md` at the same time as each annotation.

## Open questions

- Relations: should `one` also accept a required `T`, or stay `Option<T>` until
  a loader can guarantee presence?
- Relations: should a to-one relation be allowed to reference a declared
  `foreign_key` instead of repeating `table`, `local_key` and `target_key`?
- Relations: should `local_key` and `target_key` be bare identifiers or string
  literals? Bare identifiers match `unique(columns = [...])`; strings make the
  database-column reading explicit.
- Relations: should the schema test report an unknown `target_key`, or stay
  silent when the target table is owned by another service?
- Relations: should a fragment or subquery ever carry one?
- Triggers: name-only or timing/events too? Should every non-internal trigger
  be declared, or only the ones the model cares about?
- Foreign keys: is the scoped single-field form enough for now, or should the
  general composite struct-level form ship with it?
- Foreign keys: should `on delete` and `on update` be recorded and checked?
- Generated columns: should the annotation be `pg_virtual` or a neutral
  `generated`, and do stored and virtual need separate spellings?
- Unique: check `pg_index`, `pg_constraint` or both? What about partially
  indexed uniqueness?

## Acceptance checklist

For each annotation:

- The parser accepts the documented form and rejects unknown, empty, repeated
  and misplaced uses.
- The derive emits the metadata once and leaks its slices.
- The insert and patch derives omit read-only fields where relevant.
- The schema test reports missing, extra and mismatched metadata where there is
  a database counterpart.
- Unit tests cover parsing, compile-fail UI tests cover invalid use, and live
  tests cover the schema check.
- `annotations.md` documents the annotation and its generated metadata.
- At least one `invoice-service` model uses it, so the feature is exercised
  against a real schema.
