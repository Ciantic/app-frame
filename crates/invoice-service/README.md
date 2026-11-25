# Invoice service

Demonstration of invoices in the app frame; not a production billing system.

## Scope

- Store customers and invoice headers and their line items in the tenant-owned
  `customer`, `invoice` and `invoice_row` tables.
- Compute row amounts before every write and invoice totals after every row
  change with PostgreSQL triggers.
- Provide `Customer`, `Invoice` and `InvoiceRow` models with dbkit query,
  insert, patch, delete and select derives.
- Expose the schema as `invoice_service::db::schema::SCHEMA`.

## Layout

- [schema.sql](./src/db/schema.sql) — tables, the tenant-scoped foreign keys and
  the triggers that maintain the computed amounts.
- [models](./src/db/models) — the model structs and their id newtypes.
- [primitives](./src/db/primitives) — `Money`, `Quantity`, `TaxRate`, `Date`,
  `EmailAddress`, `Unit`, `Language`, the e-invoice fields and the external
  seller id.

`tenant_id` uses `TenantId` from `user-service`. Invoices reference customers
on `(tenant_id, customer_id)`, and rows reference invoices on `(tenant_id,
invoice_id)`, so a reference cannot cross a tenant. Sellers and tenants are
owned by other services, so their ids have no local foreign keys.

## Computed amounts

The source schema marks amounts as computed. PostgreSQL enforces them here, so
every writer, including a dbkit patch, sees the same values:

- A row's `net_amount` is `round(quantity * unit_price, 2)`, `tax_amount` is
  `round(net_amount * tax_rate, 2)` and `total_amount` is their sum.
- An invoice's `net_amount` and `tax_amount` are the sums of its rows, and
  `total_amount` is their sum.
- Changing a row bumps its invoice's `updated_at` and `version`.

Relations, child collections, watch subscriptions, upsert, REST routes and
default query filters from the source schema have no counterpart in this
repository yet; a query that hides trashed invoices passes `deleted_at.is_null()`
explicitly.

## Development

```bash
cargo nextest run -p invoice-service
```

The tests start a throwaway Postgres through
[pgtest-server](../pgtest-server), so podman must be available.
