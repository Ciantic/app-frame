# App-Frame project

## Guidelines for agents

- Do not navigate, or read files outside project's directory without permission
  from user.
- Do not overdocument: self-documenting code is always better. Documentation
  should be supplementary, covering only what the implementation does not make
  clear.
- This application is not in production: when making backwards incompatible
  changes, old behavior need not be referenced or maintained.

## Code guidelines

- Prefer single-line code comments. If more explanation is needed, add it to
  `docs/` and link to that.
- Avoid creating small helper functions that only add indirection. Rust is a
  verbose language. 
- Do not split logic just to make functions shorter.
- Use `cargo +nightly fmt --all` to format the code.

## Markdown guidelines

- Prefer bullet point lists over tables.

## SQLx

Docs if needed: https://docs.rs/sqlx

## Tests

- Run the tests with `cargo nextest run`

## Schema and model guidelines

- Each model should be serializable to JSON and back
- Each model is defined in Rust side as a plain struct deriving `sqlx::FromRow`
- Empty values are omitted, not `null`: every `Option` field carries
  `#[serde(skip_serializing_if = "Option::is_none")]`, so a `None` field never
  reaches the wire as `null`
- Patches are input-only and do not implement `Serialize`: a missing key leaves
  the column unchanged, `null` clears a nullable column, a value sets it
- Insert inputs are `Option<T>`: a missing nullable column is inserted as
  `NULL`, a missing defaulted column (`id`, timestamps, `version`) is filled in
  by the repository
