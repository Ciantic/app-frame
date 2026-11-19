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