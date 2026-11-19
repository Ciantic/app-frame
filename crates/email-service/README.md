# Email service

Outgoing email for the app frame. Other services insert an `Email` row and the
email service delivers it, so the table doubles as the send queue.

## Scope

- Store one row per outgoing email in the `email` table.
- Track delivery with `sent_at`: `null` means still queued.
- Provide the `Email` model to other services.
- Expose the schema as `email_service::db::schema::SCHEMA`.

Recipients, retries and provider integrations are out of scope for now.

## Layout

- [schema.sql](./src/db/schema.sql) — the `email` table.
- [models](./src/db/models) — the `Email` struct plus its id newtype. It
  serialises to JSON and derives `sqlx::FromRow`.
- [lib.rs](./src/lib.rs) — library entry point for other services.

The table keeps optimistic locking (`version`) and timestamps use `snake_case`
columns.

## Using from another service

```rust
use email_service::db::models::email::Email;
use email_service::db::models::email::EmailId;
```

The model mirrors the SQL schema, and [tests/schema.rs](./tests/schema.rs) fails
the build if the two drift apart.

## Development

```bash
cargo nextest run -p email-service
```

The tests start a throwaway Postgres through
[pgtest-server](../pgtest-server), so podman must be available.
