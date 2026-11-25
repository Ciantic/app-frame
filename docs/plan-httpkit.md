# httpkit plan

Plan for `httpkit` and `httpkit-macros`: generate axum routes for the dbkit
query, insert, patch and delete builders. Not implemented yet.

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
- Apply a default limit when `limit` is absent; `MAX_LIMIT` already rejects a
  larger explicit limit.
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
tenant column. `#[db(tenant_owned)]` is an acceptable alternative name, but
`#[db(tenant)]` fits the existing role annotations.

`#[derive(DbModel)]` records the column as `TableMeta.tenant:
Option<&'static str>` and generates `impl dbkit::TenantOwned for Widget` with
the tenant column and its Rust type. The input derives use it:

- `<Model>Insert` keeps the tenant column as a required plain field and exposes
  it through `TenantRow`.
- `<Model>Patch` makes the tenant column a required plain field, like the
  primary key and version. It is not a `Patch<T>`, so it cannot be changed. The
  patch renderer adds `AND "tenant_id" = $n` to `WHERE`.
- `<Model>PKV` includes the tenant column beside the primary key. The delete
  renderer adds it to `WHERE`.
- `<Model>PK` stays the primary key returned by insert; the caller already
  supplied the tenant.

`TenantRow` exposes the typed tenant value an input carries:

```rust
pub trait TenantRow {
    type TenantId;
    fn tenant_id(&self) -> &Self::TenantId;
}
```

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

- Insert: check `has_permission` for each entry's tenant before the insert.
- Patch: check each entry's tenant before the update; the tenant predicate
  makes the update affect only a row of that tenant.
- Delete: check each key's tenant before the delete; the tenant predicate does
  the same for delete.
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
- The extractor reads the token, hashes it, looks up `UserSession` by
  `token_hash`, and rejects revoked or expired sessions.
- `UserSession::has_permission` matches the enum and applies the global or
  tenant rule. The current two placeholder methods collapse into it.
- `UserSession::token_hash` needs `#[db(query_ops(eq))]` for the lookup.

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
pub trait Queryable: Model + Serialize + Send + Unpin + for<'r> FromRow<'r, PgRow> {
    type Column: Column<Model = Self>;
}

pub trait Insertable: Model {
    type Insert: InsertRow<Model = Self> + DeserializeOwned + Validate;
}

pub trait Patchable: Model {
    type Patch: PatchRow<Model = Self> + DeserializeOwned + Validate;
}

pub trait Deletable: Model {
    type Key: DeleteKey<Model = Self> + DeserializeOwned + Serialize;
}

pub async fn query<S, M: Queryable>(state: S, query: Query<M::Column>)
    -> ApiResult<Json<Vec<M>>>;
pub async fn insert<S, M: Insertable>(state: S, entries: Vec<M::Insert>)
    -> ApiResult<(StatusCode, Json<Vec<<M::Insert as InsertRow>::Key>>)>;
pub async fn patch<S, M: Patchable>(state: S, entries: Vec<M::Patch>)
    -> ApiResult<StatusCode>;
pub async fn delete<S, M: Deletable>(state: S, keys: Vec<M::Key>)
    -> ApiResult<StatusCode>;
```

The traits are split per operation so a model without `DbInsert`, `DbPatch` or
`DbDelete` can expose only the operations it supports. A single `Crud` trait
would force every generated type to exist even when its route is absent.

`S` implements a small `DbState` trait that returns the `PgPool`; this avoids
depending on a particular application state struct.

Validation runs before the builders. dbkit itself does not validate patch or
insert inputs.

## Macro

`httpkit-macros` provides a service-level macro rather than a derive on the
model. The same model may be mounted at different paths, with different
guards, in different binaries.

The examples use a made-up `Widget` model with an `id` primary key and a
`tenant_id` column.

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

`Widget` is tenant-owned, so the macro takes the tenant column from
`TableMeta.tenant` and checks every insert entry, patch entry and delete key
against its own tenant. A model without `#[db(tenant)]` uses global permission
for writes. `tenant_column` remains an override for a column the annotation
does not cover.

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

Generated items depend on the declared blocks:

- `impl httpkit::Queryable for Widget` only when `query` is present, naming
  `WidgetColumn`.
- `impl httpkit::Insertable for Widget` only when `insert` is present, naming
  `WidgetInsert`.
- `impl httpkit::Patchable for Widget` only when `patch` is present, naming
  `WidgetPatch`.
- `impl httpkit::Deletable for Widget` only when `delete` is present, naming
  `WidgetPKV`.
- Handler wrappers and route registration only for the declared operations.
- One `routes(state) -> Router` per entity.

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
- garde report -> `422` with field paths;
- `PatchError::Empty` -> `400`;
- `PatchError::NotUpdated` and `DeleteError::NotDeleted` -> `409`;
- constraint violations -> `409` or `422` after inspecting the database error;
- everything else -> `500`, logged, with a generic body;
- missing session -> `401`, permission denial -> `403`.

An empty insert, patch or delete batch is a no-op in dbkit; HTTP should reject
it with `400`.

Use `application/problem+json` if a standard error shape is wanted.

## Security notes

- Do not expose `user_session` through `crud!` without column restrictions;
  `token_hash` would become readable.
- Tenant isolation for writes is not covered by the query rule above.
- A missing tenant filter with global permission reads every tenant's rows, by
  design.
- Tenant filters under `or` must be rejected; otherwise the scope is not a
  bound on the result.

## Current model changes

- `Tenant` exists in `user-service` with `id: TenantId` and `name: String`.
- dbkit still needs the `#[db(tenant)]` role, `TableMeta.tenant`,
  `TenantOwned`/`TenantRow`, and tenant predicates in patch and delete.
- Primary keys still need `#[db(query_ops(eq, any))]` before HTTP reads cover
  single-row and batch lookups.
- `UserSession::token_hash` still needs `#[db(query_ops(eq))]` before the
  session extractor can use the query builder.

## Implementation order

1. Add `#[db(tenant)]`, `TableMeta.tenant`, `TenantOwned`/`TenantRow`, and the
   patch/delete tenant predicates to dbkit.
2. Add `ApiError`, `DbState`, `Session`, `Permission`, `authorize`, and
   `tenant_scope` to `httpkit`.
3. Add `Crud` and the generic query/insert/patch/delete functions.
4. Add `CurrentSession` and the `Session` implementation to `user-service`.
5. Add `query_ops` to primary keys and `token_hash`.
6. Add `httpkit-macros` and the `crud!` macro.
