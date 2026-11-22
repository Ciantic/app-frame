# User service

Small login service for the app frame. Users reach it over an axum HTTP API,
while other services depend on it as a library for the user and session models.

## Scope

The first iteration covers registration and magic link login only:

- Register a user with an email address.
- Send a magic link to that address and log in by following it.
- Issue, validate and revoke sessions for authenticated requests.
- Provide `User` and `UserSession` models to other services.
- Hand its routes to a backend binary as a reusable axum `Router`.

Passwords, OAuth providers and authorization rules are out of scope for now.

## Magic link flow

1. A client submits an email address to the registration endpoint.
2. The service creates the user if it does not exist yet and generates a
   single-use token.
3. Only the token hash is stored; the raw token goes to the user's email as a
   link.
4. Following the link verifies the token, marks the email as verified and
   creates a `user_session` row.
5. The session token authenticates later requests. Sessions are stored hashed,
   expire at `expires_at`, and are invalidated by setting `revoked_at`.

## Layout

- [schema.sql](./src/db/schema.sql) — the `user` and `user_session` tables,
  exposed as `user_service::db::schema::SCHEMA`.
- [models](./src/db/models) — `User` and `UserSession` structs plus their id
  newtypes. They serialise to JSON and derive `sqlx::FromRow`.
- [primitives](./src/db/primitives) — value types the models use, such as
  `Email`. They only carry the field's meaning: sanitization and validation are
  separate steps.
- [lib.rs](./src/lib.rs) — library entry point for other services.

Both tables keep soft delete (`deleted_at`) and optimistic locking (`version`)
columns.

## Exposing the router

The crate does not start a server or bind a port. It exports its endpoints as an
axum `Router` built from the service state, leaving composition to the binary
that serves the application.

## Using from another service

Depend on this crate by path or git and import the models:

```rust
use user_service::db::models::user::User;
use user_service::db::models::user_session::UserSession;
```

The models mirror the SQL schema, and [tests/schema.rs](./tests/schema.rs)
fails the build if the two drift apart.

## Development

```bash
cargo nextest run -p user-service
```

The tests start a throwaway Postgres through
[pgtest-server](../pgtest-server), so podman must be available.
