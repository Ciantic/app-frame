use chrono::NaiveDate;
use chrono::Utc;
use invoice_service::db::models::customer::Customer;
use invoice_service::db::models::invoice::Invoice;
use invoice_service::db::models::invoice::InvoiceId;
use invoice_service::db::models::invoice_row::InvoiceRow;
use invoice_service::db::models::invoice_row::InvoiceRowId;
use invoice_service::db::primitives::customer_id::CustomerId;
use invoice_service::db::primitives::date::Date;
use invoice_service::db::primitives::e_invoice_address::EInvoiceAddress;
use invoice_service::db::primitives::e_invoice_operator::EInvoiceOperator;
use invoice_service::db::primitives::email_address::EmailAddress;
use invoice_service::db::primitives::language::Language;
use invoice_service::db::primitives::money::Money;
use invoice_service::db::primitives::quantity::Quantity;
use invoice_service::db::primitives::tax_rate::TaxRate;
use invoice_service::db::primitives::unit::Unit;
use rust_decimal::Decimal;
use serde_json::Value;
use serde_json::json;
use user_service::db::models::tenant::TenantId;
use uuid::Uuid;

fn tenant() -> TenantId {
    TenantId(Uuid::from_u128(1))
}

fn invoice_id() -> InvoiceId {
    InvoiceId(Uuid::from_u128(2))
}

fn money(value: i64, scale: u32) -> Money {
    Money(Decimal::new(value, scale))
}

fn customer() -> Customer {
    Customer {
        id: CustomerId(Uuid::from_u128(5)),
        tenant_id: tenant(),
        name: "Acme".to_owned(),
        email: EmailAddress("billing@example.com".to_owned()),
        address: "Street 1".to_owned(),
        business_id: "FI1234567".to_owned(),
        e_invoice_address: None,
        e_invoice_operator: None,
        language: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        version: 0,
    }
}

fn invoice() -> Invoice {
    Invoice {
        id: invoice_id(),
        tenant_id: tenant(),
        number: None,
        customer_id: Some(CustomerId(Uuid::from_u128(3))),
        seller_id: None,
        language: None,
        issue_date: Some(Date(NaiveDate::from_ymd_opt(2026, 1, 15).unwrap())),
        due_date: None,
        net_amount: money(1000, 2),
        tax_amount: money(255, 2),
        total_amount: money(1255, 2),
        notes: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        deleted_at: None,
        version: 0,
    }
}

fn invoice_row() -> InvoiceRow {
    InvoiceRow {
        id: InvoiceRowId(Uuid::from_u128(4)),
        tenant_id: tenant(),
        invoice_id: invoice_id(),
        description: None,
        quantity: Some(Quantity(Decimal::new(2, 0))),
        unit: Some(Unit("h".to_owned())),
        unit_price: Some(money(1050, 2)),
        tax_rate: Some(TaxRate(Decimal::new(2550, 4))),
        net_amount: money(2100, 2),
        tax_amount: money(536, 2),
        total_amount: money(2636, 2),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        version: 0,
    }
}

#[test]
fn customer_null_optionals_are_omitted() {
    let customer = serde_json::to_value(customer()).unwrap();

    for field in ["e_invoice_address", "e_invoice_operator", "language"] {
        assert!(customer.get(field).is_none(), "{field}: {customer}");
    }
}

#[test]
fn customer_set_optionals_are_present() {
    let mut customer = customer();
    customer.e_invoice_address = Some(EInvoiceAddress("003712345678".to_owned()));
    customer.e_invoice_operator = Some(EInvoiceOperator("OP".to_owned()));
    customer.language = Some(Language("fi".to_owned()));

    let customer = serde_json::to_value(customer).unwrap();

    assert_eq!(customer["e_invoice_address"], json!("003712345678"));
    assert_eq!(customer["e_invoice_operator"], json!("OP"));
    assert_eq!(customer["language"], json!("fi"));
}

#[test]
fn null_optionals_are_omitted() {
    let invoice = serde_json::to_value(invoice()).unwrap();
    let row = serde_json::to_value(invoice_row()).unwrap();

    for field in [
        "number",
        "seller_id",
        "language",
        "due_date",
        "notes",
        "deleted_at",
    ] {
        assert!(invoice.get(field).is_none(), "{field}: {invoice}");
    }
    assert!(row.get("description").is_none(), "{row}");

    assert!(invoice.get("customer_id").is_some(), "{invoice}");
    assert!(invoice.get("issue_date").is_some(), "{invoice}");
    assert!(row.get("unit").is_some(), "{row}");
}

#[test]
fn money_serializes_as_a_decimal_string() {
    let invoice = serde_json::to_value(invoice()).unwrap();

    assert_eq!(invoice["net_amount"], json!("10.00"), "{invoice}");
    assert_eq!(invoice["total_amount"], json!("12.55"), "{invoice}");
}

#[test]
fn omitted_optionals_deserialize() {
    let json = json!({
        "id": Uuid::from_u128(2),
        "tenant_id": Uuid::from_u128(1),
        "net_amount": 0,
        "tax_amount": 0,
        "total_amount": 0,
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
        "version": 0,
    });

    let invoice: Invoice = serde_json::from_value(json).unwrap();

    assert!(invoice.number.is_none());
    assert!(invoice.customer_id.is_none());
    assert!(invoice.deleted_at.is_none());
}

#[test]
fn a_set_optional_is_present() {
    let mut invoice = invoice();
    invoice.notes = Some("Thank you".to_owned());

    let invoice = serde_json::to_value(invoice).unwrap();

    assert!(
        matches!(invoice.get("notes"), Some(Value::String(_))),
        "{invoice}"
    );
}
