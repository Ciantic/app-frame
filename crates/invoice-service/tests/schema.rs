use invoice_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;

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
