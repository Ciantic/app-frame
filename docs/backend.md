# Backend

Notes for a future `backend` crate: a binary that serves all services from one
process.

## Composing routers

Each service crate exports an axum `Router` and never starts a server or binds a
port itself. The binary mounts them:

```rust
use axum::Router;

let app = Router::new()
    .nest("/user", user_service::router(state))
    .nest("/other", other_service::router(state));

axum::serve(listener, app).await?;
```

## Conventions for service routers

- Routes inside a service router are relative (`/register`, `/login/magic-link`),
  so the mounting prefix decides the public paths.
- The router takes the state it needs as an argument; a binary that shares one
  state struct across services can implement `axum::extract::FromRef` for the
  narrower per-service state instead.
- Because the router is plain data, the same service can be mounted at different
  prefixes in different binaries, and tests can build it without a server
  process.
