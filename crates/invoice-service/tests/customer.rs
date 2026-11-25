use dbkit::InsertError;
use invoice_service::db::models::customer::Customer;
use invoice_service::db::models::customer::CustomerInsert;
use invoice_service::db::models::invoice::Invoice;
use invoice_service::db::models::invoice::InvoiceInsert;
use invoice_service::db::primitives::customer_id::CustomerId;
use invoice_service::db::primitives::e_invoice_address::EInvoiceAddress;
use invoice_service::db::primitives::email_address::EmailAddress;
use invoice_service::db::primitives::language::Language;
use invoice_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;
use sqlx::PgPool;
use user_service::db::models::tenant::TenantId;
use uuid::Uuid;

fn email(text: &str) -> EmailAddress {
    EmailAddress(text.to_owned())
}

async fn database() -> PgPool {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();
    db
}

async fn customer(db: &PgPool, id: CustomerId) -> Customer {
    Customer::query()
        .filters([Customer::columns().id.eq(id)])
        .build()
        .unwrap()
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_customer_takes_database_defaults_and_keeps_optionals() {
    let db = database().await;
    let tenant = TenantId(Uuid::from_u128(30));

    let keys = Customer::insert([CustomerInsert::new(
        tenant,
        "Acme",
        email("billing@example.com"),
        "Street 1",
        "FI1234567",
    )
    .set_e_invoice_address(EInvoiceAddress("003712345678".to_owned()))
    .set_language(Language("fi".to_owned()))])
    .execute(&db)
    .await
    .unwrap();

    let customer = customer(&db, keys[0].id).await;

    assert_eq!(customer.name, "Acme");
    assert_eq!(customer.email, email("billing@example.com"));
    assert_eq!(
        customer.e_invoice_address,
        Some(EInvoiceAddress("003712345678".to_owned()))
    );
    assert!(customer.e_invoice_operator.is_none());
    assert_eq!(customer.language, Some(Language("fi".to_owned())));
    assert_eq!(customer.version, 0);
    assert!(customer.created_at <= chrono::Utc::now());
}

#[tokio::test]
async fn an_invoice_cannot_reference_another_tenants_customer() {
    let db = database().await;
    let tenant_a = TenantId(Uuid::from_u128(40));
    let tenant_b = TenantId(Uuid::from_u128(41));

    let keys = Customer::insert([CustomerInsert::new(
        tenant_a,
        "Acme",
        email("billing@example.com"),
        "Street 1",
        "FI1234567",
    )])
    .execute(&db)
    .await
    .unwrap();
    let customer_id = keys[0].id;

    let keys = Invoice::insert([InvoiceInsert::new(tenant_a).set_customer_id(customer_id)])
        .execute(&db)
        .await
        .unwrap();
    let invoice_id = keys[0].id;

    let invoice = Invoice::query()
        .filters([Invoice::columns().id.eq(invoice_id)])
        .build()
        .unwrap()
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(invoice.customer_id, Some(customer_id));

    let error = Invoice::insert([InvoiceInsert::new(tenant_b).set_customer_id(customer_id)])
        .execute(&db)
        .await
        .unwrap_err();
    assert!(matches!(error, InsertError::Sqlx(_)), "{error}");
}
