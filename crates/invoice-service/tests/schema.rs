use invoice_service::db::models::MODELS;
use invoice_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;

#[tokio::test]
async fn models_and_schema_share_columns() {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();
    let mut connection = db.acquire().await.unwrap();

    let problems = dbkit::schema::check_live_schema_against_models(&mut connection, &MODELS)
        .await
        .unwrap();

    assert!(
        problems.is_empty(),
        "model/schema mismatch:\n{}",
        problems.join("\n")
    );
}
