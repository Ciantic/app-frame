# httpkit next steps

Companion to [httpkit](../httpkit.md). The core is implemented and tested, but
no service mounts generated routes yet, no real model is tenant-owned, and the
permission methods are still placeholders.

## Before exposing any route

- Implement `UserSession::has_global_permissions` and
  `has_tenant_permissions`; both currently return `true`.
- Add at least one real `#[db(tenant)]` model and its `tenant_id` schema column,
  foreign key and index.
- Build a service router and mount it in a binary; `httpkit` generates a
  module, but `user-service` does not export a router yet.
- Decide which models get `crud!` at all. `user_session` should stay private
  because `token_hash` is readable through a query endpoint.
- Add a batch-size limit so one request cannot open an unbounded transaction.
- Confirm the axum body limit and configure it explicitly for write endpoints.

## Permissions

- Replace the two placeholder boolean methods with real role or scope checks.
- Consider typed scope/action constants instead of raw strings.
- Decide whether a tenant-owned model may expose a deliberately global write;
  today a missing tenant value falls back to global permission.
- Decide whether a tenant-scoped session must always include a tenant filter,
  instead of relying on `has_global_permissions` being false.
- Add tests for every scope/action pair, including denies and tenant
  mismatches.
- Decide whether a denied tenant filter should be `400` for an invalid shape or
  `403` for a permission failure.

## Authentication

- Implement login and cookie issuance; `CurrentSession` only reads an existing
  session.
- Set `HttpOnly`, `Secure`, `SameSite` and an expiry on the session cookie.
- Add CSRF protection for cookie-authenticated writes if the app is
  browser-facing.
- Add session rotation, logout/revocation and cleanup of expired rows.
- Decide the intended bearer-token use: API keys, machine sessions or the same
  session token. `CurrentSession` currently treats it as the same token.
- Add tests for expired, revoked and malformed cookies and bearer headers.
- Review whether SHA-256 of a high-entropy token is enough or whether the token
  should be generated and stored through a dedicated primitive.

## Testing

- Add HTTP integration tests with `tower::ServiceExt::oneshot` and a test
  session extractor, so routes can be tested without login.
- Add live tests for cross-tenant writes: a key carrying tenant A must not
  update or delete tenant B's row.
- Add tests for a tenant filter under `or`, an empty tenant list, and a
  non-`eq`/`any` tenant operator.
- Add compile-fail tests for `crud!` with a declared operation whose derive is
  missing.
- Add router tests for every opt-in combination of query, insert, patch and
  delete.
- Once login exists, add the end-to-end tests that are currently out of scope.

## HTTP surface

- `GET /{entity}?q=<json>` can hit URL length limits; consider
  `POST /{entity}/query` with the same JSON body.
- Projections are rejected; add opt-in `select` support through `DbSelect` and
  `build_selected`.
- Decide whether list responses need a page envelope or a total count. dbkit
  currently has no count query.
- Insert returns keys only. Decide whether a client needs the inserted rows or a
  follow-up `GET` is enough.
- Add a stable error `code` for clients; problem+json currently carries only
  title, status and detail.
- Map database constraint violations to field-level errors where possible.
- Render garde reports as structured field errors instead of one detail string.

## Macro and routing

- Allow an explicit generated module name, or at least document the
  `httpkit_routes_<path>` convention.
- Allow per-operation paths or a path override if entities need nested
  resources.
- Consider a pre-body `Can<...>` extractor for route-level authorization; the
  current write checks need the body to know the tenant.
- Add the `Route` metadata or registry only if OpenAPI generation or route
  listing becomes a requirement.
- Consider a test-only helper that mounts generated routes with a fixed
  session.

## Operations

- Add tracing spans around generated handlers with table, action and tenant
  fields.
- Add metrics for request count, deny count, conflict count and database
  duration.
- Add rate limiting and CORS where the service is exposed beyond local use.
- Decide whether a failed batch should keep returning the first failing index
  or a fuller report.
- Consider batch size and statement count limits for insert, patch and delete.

## Documentation

- Link `docs/httpkit.md` from the project or backend documentation.
- If the document grows, split it into `docs/httpkit/overview.md`,
  `docs/httpkit/permissions.md` and `docs/httpkit/macros.md`.
- Document the `model` path rule: the generated `ModelInsert`, `ModelPatch`,
  `ModelPKV` and `ModelColumn` types are found by suffixing the model's last
  path segment.

## First-exposure checklist

- Real permission checks are in place.
- A real tenant-owned table and `crud!` route exist.
- `user_service::router` is mounted in a binary.
- Cross-tenant writes are covered by live tests.
- Session cookies are `HttpOnly`, `Secure` and `SameSite`.
- A test-session route integration test exists.
- No sensitive model such as `user_session` is exposed.
