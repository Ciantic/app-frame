use dbkit::InsertError;
use invoice_service::db::models::invoice::Invoice;
use invoice_service::db::models::invoice::InvoiceInsert;
use invoice_service::db::models::seller::Seller;
use invoice_service::db::models::seller::SellerInsert;
use invoice_service::db::primitives::e_invoice_address::EInvoiceAddress;
use invoice_service::db::primitives::language::Language;
use invoice_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;
use sqlx::PgPool;
use user_service::db::models::tenant::TenantId;
use uuid::Uuid;

async fn database() -> PgPool {
    let db = PgtestServer::start().await.database().await;
    SCHEMA.apply(&db).await.unwrap();
    db
}

async fn ensure_tenant(db: &PgPool, id: TenantId) {
    sqlx::query(
        r#"insert into "tenant" ("id", "name") values ($1, $2) on conflict ("id") do nothing"#,
    )
    .bind(id)
    .bind("test")
    .execute(db)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_seller_takes_database_defaults_and_keeps_optionals() {
    let db = database().await;
    let tenant = TenantId(Uuid::from_u128(50));
    ensure_tenant(&db, tenant).await;

    let keys = Seller::insert([SellerInsert::new(tenant, "Acme Oy", "FI1234567")
        .set_e_invoice_address(EInvoiceAddress("003712345678".to_owned()))
        .set_language(Language("fi".to_owned()))])
    .execute(&db)
    .await
    .unwrap();

    let seller = Seller::query()
        .filters([Seller::columns().id.eq(keys[0].id)])
        .build()
        .unwrap()
        .fetch_one(&db)
        .await
        .unwrap();

    assert_eq!(seller.name, "Acme Oy");
    assert_eq!(seller.business_id, "FI1234567");
    assert_eq!(
        seller.e_invoice_address,
        Some(EInvoiceAddress("003712345678".to_owned()))
    );
    assert!(seller.e_invoice_operator.is_none());
    assert_eq!(seller.language, Some(Language("fi".to_owned())));
    assert_eq!(seller.version, 0);
    assert!(seller.created_at <= chrono::Utc::now());
}

#[tokio::test]
async fn an_invoice_cannot_reference_another_tenants_seller() {
    let db = database().await;
    let tenant_a = TenantId(Uuid::from_u128(60));
    let tenant_b = TenantId(Uuid::from_u128(61));
    ensure_tenant(&db, tenant_a).await;
    ensure_tenant(&db, tenant_b).await;

    let keys = Seller::insert([SellerInsert::new(tenant_a, "Acme Oy", "FI1234567")])
        .execute(&db)
        .await
        .unwrap();
    let seller_id = keys[0].id;

    Invoice::insert([InvoiceInsert::new(tenant_a).set_seller_id(seller_id)])
        .execute(&db)
        .await
        .unwrap();

    let error = Invoice::insert([InvoiceInsert::new(tenant_b).set_seller_id(seller_id)])
        .execute(&db)
        .await
        .unwrap_err();
    assert!(matches!(error, InsertError::Sqlx(_)), "{error}");
}
