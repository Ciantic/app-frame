use invoice_service::db::models::invoice::Invoice;
use invoice_service::db::models::invoice::InvoiceId;
use invoice_service::db::models::invoice::InvoiceInsert;
use invoice_service::db::models::invoice::InvoicePKV;
use invoice_service::db::models::invoice::InvoicePatch;
use invoice_service::db::models::invoice_row::InvoiceRow;
use invoice_service::db::models::invoice_row::InvoiceRowId;
use invoice_service::db::models::invoice_row::InvoiceRowInsert;
use invoice_service::db::models::invoice_row::InvoiceRowPKV;
use invoice_service::db::models::invoice_row::InvoiceRowPatch;
use invoice_service::db::primitives::money::Money;
use invoice_service::db::primitives::quantity::Quantity;
use invoice_service::db::primitives::tax_rate::TaxRate;
use invoice_service::db::primitives::unit::Unit;
use invoice_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;
use rust_decimal::Decimal;
use sqlx::PgPool;
use user_service::db::models::tenant::TenantId;
use uuid::Uuid;

fn money(value: i64, scale: u32) -> Money {
    Money(Decimal::new(value, scale))
}

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

async fn invoice(db: &PgPool, id: InvoiceId) -> Invoice {
    Invoice::query()
        .filters([Invoice::columns().id.eq(id)])
        .build()
        .unwrap()
        .fetch_one(db)
        .await
        .unwrap()
}

async fn invoice_row(db: &PgPool, id: InvoiceRowId) -> InvoiceRow {
    InvoiceRow::query()
        .filters([InvoiceRow::columns().id.eq(id)])
        .build()
        .unwrap()
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn computed_amounts_follow_row_writes() {
    let db = database().await;
    let tenant = TenantId(Uuid::from_u128(10));
    ensure_tenant(&db, tenant).await;

    let keys = Invoice::insert([InvoiceInsert::new(tenant).set_number("INV-1")])
        .execute(&db)
        .await
        .unwrap();
    let invoice_id = keys[0].id;

    let keys = InvoiceRow::insert([InvoiceRowInsert::new(tenant, invoice_id)
        .set_description("Consulting")
        .set_quantity(Quantity(Decimal::new(2, 0)))
        .set_unit(Unit("h".to_owned()))
        .set_unit_price(money(1050, 2))
        .set_tax_rate(TaxRate(Decimal::new(2550, 4)))])
    .execute(&db)
    .await
    .unwrap();
    let row_id = keys[0].id;

    let row = invoice_row(&db, row_id).await;
    assert_eq!(row.net_amount, money(2100, 2));
    assert_eq!(row.tax_amount, money(536, 2));
    assert_eq!(row.total_amount, money(2636, 2));
    assert_eq!(row.version, 0);

    let header = invoice(&db, invoice_id).await;
    assert_eq!(header.net_amount, money(2100, 2));
    assert_eq!(header.tax_amount, money(536, 2));
    assert_eq!(header.total_amount, money(2636, 2));
    assert_eq!(header.version, 1);

    InvoiceRow::patch([InvoiceRowPatch::new(row_id, tenant, row.version)
        .set_quantity(Quantity(Decimal::new(3, 0)))])
    .execute(&db)
    .await
    .unwrap();

    let row = invoice_row(&db, row_id).await;
    assert_eq!(row.net_amount, money(3150, 2));
    assert_eq!(row.tax_amount, money(803, 2));
    assert_eq!(row.total_amount, money(3953, 2));
    assert_eq!(row.version, 1);

    let header = invoice(&db, invoice_id).await;
    assert_eq!(header.net_amount, money(3150, 2));
    assert_eq!(header.tax_amount, money(803, 2));
    assert_eq!(header.total_amount, money(3953, 2));
    assert_eq!(header.version, 2);

    Invoice::patch([
        InvoicePatch::new(invoice_id, tenant, header.version).set_notes("recalculated")
    ])
    .execute(&db)
    .await
    .unwrap();

    let header = invoice(&db, invoice_id).await;
    assert_eq!(header.net_amount, money(3150, 2));
    assert_eq!(header.total_amount, money(3953, 2));
    assert_eq!(header.version, 3);

    InvoiceRow::delete([InvoiceRowPKV {
        id: row_id,
        tenant_id: tenant,
        version: row.version,
    }])
    .execute(&db)
    .await
    .unwrap();

    let header = invoice(&db, invoice_id).await;
    assert_eq!(header.net_amount, money(0, 2));
    assert_eq!(header.tax_amount, money(0, 2));
    assert_eq!(header.total_amount, money(0, 2));
    assert_eq!(header.version, 4);

    let rows = InvoiceRow::query()
        .filters([InvoiceRow::columns().id.eq(row_id)])
        .build()
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn deleting_an_invoice_cascades_its_rows() {
    let db = database().await;
    let tenant = TenantId(Uuid::from_u128(20));
    ensure_tenant(&db, tenant).await;

    let keys = Invoice::insert([InvoiceInsert::new(tenant)])
        .execute(&db)
        .await
        .unwrap();
    let invoice_id = keys[0].id;

    let keys = InvoiceRow::insert([InvoiceRowInsert::new(tenant, invoice_id)])
        .execute(&db)
        .await
        .unwrap();
    let row_id = keys[0].id;

    let header = invoice(&db, invoice_id).await;
    Invoice::delete([InvoicePKV {
        id: invoice_id,
        tenant_id: tenant,
        version: header.version,
    }])
    .execute(&db)
    .await
    .unwrap();

    let invoices = Invoice::query()
        .filters([Invoice::columns().id.eq(invoice_id)])
        .build()
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(invoices.is_empty());

    let rows = InvoiceRow::query()
        .filters([InvoiceRow::columns().id.eq(row_id)])
        .build()
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(rows.is_empty());
}
