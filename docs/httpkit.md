# httpkit

How `httpkit` and `httpkit-macros` generate axum routes for the dbkit query,
insert, patch and delete builders. Implemented in `crates/httpkit`,
`crates/httpkit-macros` and the dbkit tenant support.

## Scope

- One read endpoint per entity: a query endpoint that uses `dbkit::Query`.
- One insert, one patch and one delete endpoint per entity.
- No separate list/get endpoints: a client filters by primary key with `eq` for
  one row and `any` for a batch.
- Permission checks go through `UserSession::has_permission`, which takes an
  `httpkit::Permission` and handles both global and tenant cases.
- `httpkit` stays independent of `user-service`; the service supplies the
  session extractor and the `Session` implementation.

## Routes

Every entity uses one path and exposes only the operations declared in
`crud!`:

- `GET /{entity}?q=<query json>` returns `200 Vec<Model>`.
- `POST /{entity}` takes `Vec<ModelInsert>` and returns `201 Vec<ModelPK>`.
- `PATCH /{entity}` takes `Vec<ModelPatch>` and returns `204`.
- `DELETE /{entity}` takes `Vec<ModelPKV>` and returns `204`.

Delete has no item route: one `DELETE` with a vector of keys. Batching matches
the dbkit builders and avoids a second path shape.

## Query endpoint

- Input is one URL parameter holding the dbkit query JSON:
  `GET /widgets?q={"filters":{"tenant_id":"...","id":"..."},"limit":20}`.
- `httpkit` parses it with `Query::<ModelColumn>::parse`.
- A missing `q` means no filters, ordering or paging.
- Apply `httpkit::DEFAULT_LIMIT` (1000) when `limit` is absent; `MAX_LIMIT`
  already rejects a larger explicit limit.
- Reject `select` in the first version and return full rows. Projections need
  `DbSelect` and a response type decision, so they are separate work.
- Return `Vec<Model>`, not a page envelope. Total count is not part of the
  query builder.

Primary keys get `#[db(query_ops(eq, any))]`; on the example model that is
`Widget.id`. One endpoint then covers both single-row and batch reads:

- `{"filters":{"id":"018f..."}}` reads one row.
- `{"filters":{"id__any":["018f...","0190..."]}}` reads several.

## Permission model

Permission depends on the request, not just the route. The query handler runs:

1. Extract `CurrentSession`; a missing or expired session is `401`.
2. Parse `q`; malformed input is `400`.
3. Derive the tenant scope from the filters.
4. Call `has_permission`; denial is `403`.
5. Build and execute the query.

The tenant rule:

- A tenant filter in the query means tenant-specific permission.
- No tenant filter means global permission.
- A tenant filter under `or` is rejected: `{"or":[{"tenant_id":"a"},{"id":"b"}]}`
  would otherwise return rows outside tenant `a` while only checking `a`.
- Only `eq` and `any` on the tenant column count as a scope. `neq`, `is_null`,
  `between` and the rest are rejected as tenant selectors.
- `any` may name several tenants; every tenant must pass `has_permission` with
  its tenant permission.
- A model without a tenant column cannot produce a tenant scope, so its query
  is global.

The helper lives in `httpkit` and walks the public filter tree:

```rust
enum TenantScope<T> {
    Global,
    Tenants(Vec<T>),
}

fn tenant_scope<C: Column, T: DeserializeOwned>(
    filters: &[Filter<C>],
    tenant_column: &str,
) -> Result<TenantScope<T>, ApiError>;
```

Values are read through `Operand::One` and `Operand::List` using
`Bind::as_json()` and deserialized into `T`. In `user-service`, `T` is
`TenantId`, so the filter value goes through the same newtype deserialization
as the rest of the API.

## Tenant-owned models

A table that belongs to a tenant marks its ownership column with a dbkit role:

```rust
#[db(tenant)]
#[db(pgtype = "uuid")]
pub tenant_id: TenantId,
```

The role is legal on one table column, not on a fragment or subquery. The
tenant column must be non-null and must not have a default, so every write
states its tenant explicitly; the derive rejects a nullable or defaulted
tenant column. The column may also be `#[db(primary_key)]`; the predicate is
then already in the key and is not rendered a second time.
`#[db(tenant_owned)]` is an acceptable alternative name, but `#[db(tenant)]`
fits the existing role annotations.

`#[derive(DbModel)]` records the column as `TableMeta.tenant:
Option<&'static str>` and generates `impl dbkit::TenantOwned for Widget` with
the tenant column. The input derives use it:

- `<Model>Insert` keeps the tenant column as a required plain field and exposes
  it through `TenantRow`.
- `<Model>Patch` makes the tenant column a required plain field, like the
  primary key and version. It is not a `Patch<T>`, so it cannot be changed. The
  patch renderer adds `AND "tenant_id" = $n` to `WHERE`.
- `<Model>PKV` includes the tenant column beside the primary key, unless the
  key already contains it. The delete renderer adds it to `WHERE`.
- `<Model>PK` stays the primary key returned by insert; the caller already
  supplied the tenant.

`TenantRow` exposes the tenant value an input carries, without making the
macro branch on whether the model is tenant-owned:

```rust
pub trait TenantRow {
    fn tenant(&self) -> Option<&dyn Bind>;
}
```

`InsertRow`, `PatchRow` and `DeleteKey` require `TenantRow`. Every generated
insert, patch and key type implements it: a tenant-owned model returns
`Some(&self.tenant_id)`, a model without `#[db(tenant)]` returns `None`. The
renderers append the tenant predicate whenever `tenant()` is `Some`, and the
HTTP permission helper parses the value into `Session::TenantId` through
`Bind::as_json()`.

This keeps `#[db(tenant)]` as the single source of truth. The macro cannot see
the model's derives or annotations, so a runtime `Option` is what lets one
generated wrapper work for both tenant-owned and global models.

The generated patch SQL becomes:

```sql
UPDATE "widget"
SET "name" = $1, "version" = "version" + 1, "updated_at" = now()
WHERE "id" = $2 AND "tenant_id" = $3 AND "version" = $4
```

Checking a tenant id from the request body is not sufficient by itself. If the
tenant is not part of the row predicate, a caller authorized for tenant A can
claim A in the request and still target tenant B's row by primary key. The
predicate makes that write affect zero rows, which becomes `409 Conflict`.

## Writes and tenancy

`httpkit` checks a write batch through one helper:

```rust
fn authorize_rows<S: Session, T: TenantRow>(
    session: &S,
    rows: &[T],
    scope: &str,
    action: &str,
) -> Result<(), ApiError>
where
    S::TenantId: DeserializeOwned;
```

For each row it reads `TenantRow::tenant()`:

- `Some(value)` checks a tenant permission after deserializing the value into
  `S::TenantId`;
- `None` checks a global permission.

Then:

- Insert: check entries before the insert.
- Patch: check entries before the update; the tenant predicate makes the
  update affect only a row of that tenant.
- Delete: check keys before the delete; the tenant predicate does the same for
  delete.
- Query: derive `tenant_column` from `TableMeta.tenant` instead of repeating it
  in `crud!`.

A batch may span tenants. Every entry or key must pass its own permission
check; one denial rejects the whole batch before any SQL runs.

## Session and permissions

`httpkit` defines the session contract, without knowing how to load a session:

```rust
pub trait Session {
    type TenantId;

    fn has_permission(&self, permission: &Permission<Self::TenantId>) -> bool;
}

pub enum Permission<'a, T> {
    Global { scope: &'a str, action: &'a str },
    Tenant { tenant_id: T, scope: &'a str, action: &'a str },
}

pub fn authorize<S: Session>(
    session: &S,
    permission: &Permission<'_, S::TenantId>,
) -> Result<(), ApiError>;
```

One method is enough because the enum already separates global from tenant
permission. `authorize` becomes a thin mapping from `false` to `403`, and a new
permission variant later does not grow the session trait.

The associated type keeps `httpkit` independent of `user-service`. The service
picks `TenantId`; `httpkit` only requires whatever type it picks to deserialize
from a filter value.

`user-service` owns the extractor and implements the trait for its session:

- `CurrentSession(pub UserSession)` implements `FromRequestParts<AppState>`.
- It reads `Cookie: session=...` first and falls back to
  `Authorization: Bearer ...`.
- It hashes the raw token with SHA-256 and compares the lowercase hex against
  `user_session.token_hash`, then rejects revoked or expired sessions.
- `UserSession::has_permission` matches the enum and applies the global or
  tenant rule.
- `UserSession::token_hash` carries `#[db(query_ops(eq))]` for the lookup.

```rust
impl httpkit::Session for UserSession {
    type TenantId = crate::db::models::tenant::TenantId;

    fn has_permission(&self, permission: &httpkit::Permission<TenantId>) -> bool {
        match permission {
            httpkit::Permission::Global { scope, action } => {
                self.has_global_permissions(scope, action)
            }
            httpkit::Permission::Tenant {
                tenant_id,
                scope,
                action,
            } => self.has_tenant_permissions(*tenant_id, scope, action),
        }
    }
}
```

The two named methods above are implementation details of `UserSession`, not
part of the `Session` contract; they can also be inlined into `has_permission`.
The public session API is the single enum-based method.

The sketch currently returns `true`; the real checks must land before any route
is exposed.

## Runtime API

`httpkit` owns the operation logic as generic functions; the macro generates
only wiring.

```rust
pub async fn query<S, C>(state: &S, query: Query<C>)
    -> ApiResult<Json<Vec<C::Model>>>;
pub async fn insert<S, P>(state: &S, entries: Vec<P>)
    -> ApiResult<(StatusCode, Json<Vec<P::Key>>)>;
pub async fn patch<S, P>(state: &S, entries: Vec<P>) -> ApiResult<StatusCode>;
pub async fn delete<S, K>(state: &S, keys: Vec<K>) -> ApiResult<StatusCode>;
```

The generated handler names the concrete column, insert, patch and key types.
The functions are not constrained by per-model capability traits: the same
model may be mounted by several `crud!` invocations, and each generated trait
impl would collide.

`S` implements a small `DbState` trait that returns the `PgPool`; this avoids
depending on a particular application state struct.

Validation runs before the builders. dbkit itself does not validate patch or
insert inputs.

## Macro

`httpkit-macros` provides a service-level macro rather than a derive on the
model. The same model may be mounted at different paths, with different
guards, in different binaries.

The examples use a made-up `Widget` model with an `id` primary key and a
`tenant_id` column. `model` is the model's path; the generated insert, patch,
key and column types are found by suffixing its last segment in the same
module, so pass a qualified path when the model is imported from elsewhere.

```rust
httpkit::crud! {
    model = Widget,
    path = "/widgets",
    state = AppState,
    session = crate::auth::CurrentSession,

    query  { scope = "widget", action = "read" },
    insert { scope = "widget", action = "create" },
    patch  { scope = "widget", action = "update" },
    delete { scope = "widget", action = "delete" },
}
```

`Widget` is tenant-owned. The macro calls `authorize_rows` for every write
batch; each entry or key decides through `TenantRow` whether it carries a
tenant. A model without `#[db(tenant)]` returns `None` and falls back to global
permission. `TableMeta.tenant` supplies the query column, so `tenant_column` is
only a query-side override; tenant-owned writes require the annotation.

Operations are opt-in. A block that is absent generates no route, no handler
and no trait impl, so a model can expose a subset:

```rust
httpkit::crud! {
    model = Widget,
    path = "/widgets",
    state = AppState,
    session = crate::auth::CurrentSession,

    query  { scope = "widget", action = "read" },
    insert { scope = "widget", action = "create" },
}
```

This mounts `GET` and `POST` only; `PATCH` and `DELETE` are not exposed. The
model needs `DbInsert` for the insert block, but does not need `DbPatch` or
`DbDelete`. At least one operation block is required. A declared block whose
derived type is missing, such as `insert` without `DbInsert`, fails at the
missing `WidgetInsert`; the macro only sees the model name, not its derives.

The macro expands to a module named from the path, for example
`httpkit_routes_widgets`, containing:

- Handler wrappers only for the declared operations, naming `WidgetColumn`,
  `WidgetInsert`, `WidgetPatch` or `WidgetPKV` as needed.
- `routes(state) -> Router`, which registers one method router on the path.

A model may be mounted repeatedly at different paths because the macro does
not generate per-model trait impls.

The query wrapper checks permission after parsing, because the tenant scope is
inside the query payload. Write wrappers check each entry or key after parsing
the body and before the database call; if pre-body authorization matters, a
`Can<...>` extractor can run before `Json`.

## Route trait

No handler or guard fields on `Route`. Handlers have different extractor
types, and the query permission depends on parsed data, so neither fits a
uniform field. Keep routing as plain axum:

- each generated route is a concrete `MethodRouter<S>`;
- `Route<S>` may remain a thin metadata struct with method, path and router, or
  be omitted entirely;
- an object-safe trait with `fn into_router(self: Box<Self>)` is only needed
  for a runtime registry or OpenAPI generation later.

## Errors

`httpkit` defines one `ApiError` implementing `IntoResponse`:

- query parse/render errors -> `400`;
- garde report -> `422` with the report text;
- `PatchError::Empty` -> `400`;
- `PatchError::NotUpdated` and `DeleteError::NotDeleted` -> `409`;
- constraint violations -> `409` or `422` after inspecting the database error;
- everything else -> `500`, logged, with a generic body;
- missing session -> `401`, permission denial -> `403`.

An empty insert, patch or delete batch is rejected with `400`.

Every error is `application/problem+json`.

## Security notes

- Do not expose `user_session` through `crud!` without column restrictions;
  `token_hash` would become readable.
- Writes are tenant-isolated by the tenant predicate. Query isolation still
  depends on the tenant-filter rule above.
- A missing tenant filter with global permission reads every tenant's rows, by
  design.
- Tenant filters under `or` must be rejected; otherwise the scope is not a
  bound on the result.

## Decisions

- Default query limit is `httpkit::DEFAULT_LIMIT` = 1000.
- Errors use `application/problem+json`.
- `CurrentSession` reads `Cookie: session=...`, then falls back to
  `Authorization: Bearer ...`, and compares a SHA-256 hex hash.
- `#[db(tenant)]` may share a column with `#[db(primary_key)]`; the tenant
  predicate is skipped when the primary key already contains it.
- Patch and delete order predicates as primary key, tenant, version.

## Implemented

- dbkit has `#[db(tenant)]`, `TableMeta.tenant`, `TenantOwned`, `TenantRow`,
  tenant-aware keys and tenant predicates in patch and delete.
- `Tenant` exists in `user-service` with `id: TenantId` and `name: String`.
- Primary keys carry `#[db(query_ops(eq, any))]`; `token_hash` carries
  `#[db(query_ops(eq))]`.
- `httpkit` has `ApiError`, `DbState`, `Session`, `Permission`,
  `tenant_scope`, `authorize_query`, `authorize_rows`, the generic operations
  and the extractors.
- `user-service` has `AppState`, `CurrentSession` and the `Session`
  implementation.
- `httpkit-macros` provides `crud!`, including opt-in operations.

## Remaining work

- Replace the placeholder permission methods with real authorization.
- Decide whether a tenant model may expose writes that are deliberately global.
- Add end-to-end HTTP tests once login exists; current tests cover dbkit
  rendering, permission helpers and router construction.
