use invoice_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;

#[test]
fn schema_sql_is_generated() {
    assert_eq!(
        SCHEMA.generate().unwrap(),
        SCHEMA.sql,
        "regenerate with `cargo run -p invoice-service --bin invoice-schema`"
    );
}

#[tokio::test]
async fn models_and_schema_share_columns() {
    let db = PgtestServer::start().await.database().await;
    SCHEMA.apply(&db).await.unwrap();
    let mut connection = db.acquire().await.unwrap();

    let problems = SCHEMA.check(&mut connection).await.unwrap();

    assert!(
        problems.is_empty(),
        "model/schema mismatch:\n{}",
        problems.join("\n")
    );
}
