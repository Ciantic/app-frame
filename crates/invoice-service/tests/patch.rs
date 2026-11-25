use dbkit::Patch;
use garde::Validate;
use invoice_service::db::models::customer::CustomerId;
use invoice_service::db::models::customer::CustomerPatch;
use invoice_service::db::models::invoice::InvoicePatch;
use invoice_service::db::models::invoice_row::InvoiceRowPatch;
use invoice_service::db::primitives::date::Date;
use invoice_service::db::primitives::e_invoice_address::EInvoiceAddress;
use invoice_service::db::primitives::email_address::EmailAddress;
use invoice_service::db::primitives::language::Language;
use invoice_service::db::primitives::unit::Unit;
use serde_json::json;
use user_service::db::models::tenant::TenantId;
use uuid::Uuid;

fn id() -> Uuid {
    Uuid::from_u128(1)
}

fn tenant() -> Uuid {
    Uuid::from_u128(2)
}

#[test]
fn customer_patch_requires_id_tenant_and_version() {
    assert!(
        serde_json::from_value::<CustomerPatch>(json!({ "id": id(), "tenant_id": tenant() }))
            .is_err()
    );
    assert!(serde_json::from_value::<CustomerPatch>(json!({ "id": id(), "version": 3 })).is_err());
    assert!(
        serde_json::from_value::<CustomerPatch>(json!({ "tenant_id": tenant(), "version": 3 }))
            .is_err()
    );
}

#[test]
fn customer_patch_validates_set_values() {
    let valid: CustomerPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "email": "billing@example.com",
        "e_invoice_address": "003712345678",
        "language": "fi",
    }))
    .unwrap();

    assert!(valid.validate().is_ok());
    assert_eq!(
        valid.email,
        Patch::Set(EmailAddress("billing@example.com".to_owned()))
    );
    assert_eq!(
        valid.e_invoice_address,
        Patch::Set(EInvoiceAddress("003712345678".to_owned()))
    );
    assert_eq!(valid.language, Patch::Set(Language("fi".to_owned())));

    let invalid_email: CustomerPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "email": "nope",
    }))
    .unwrap();
    let report = invalid_email.validate().unwrap_err();
    assert!(report.to_string().starts_with("email"), "{report}");

    let invalid_address: CustomerPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "e_invoice_address": "",
    }))
    .unwrap();
    let report = invalid_address.validate().unwrap_err();
    assert!(
        report.to_string().starts_with("e_invoice_address"),
        "{report}"
    );
}

#[test]
fn customer_patch_clears_optionals() {
    let patch: CustomerPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "e_invoice_address": null,
        "language": null,
    }))
    .unwrap();

    assert_eq!(patch.e_invoice_address, Patch::Null);
    assert_eq!(patch.language, Patch::Null);
    assert!(patch.validate().is_ok());
}

#[test]
fn invoice_patch_reads_all_three_states() {
    let patch: InvoicePatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 3,
        "number": "INV-1",
        "customer_id": Uuid::from_u128(4),
        "notes": null,
        "issue_date": "2026-01-15",
    }))
    .unwrap();

    assert_eq!(patch.number, Patch::Set("INV-1".to_owned()));
    assert_eq!(
        patch.customer_id,
        Patch::Set(CustomerId(Uuid::from_u128(4)))
    );
    assert_eq!(patch.notes, Patch::Null);
    assert_eq!(patch.due_date, Patch::Absent);
    assert_eq!(
        patch.issue_date,
        Patch::Set(Date(chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()))
    );
}

#[test]
fn invoice_patch_omits_absent_fields() {
    let patch = InvoicePatch::new(
        invoice_service::db::models::invoice::InvoiceId(id()),
        TenantId(tenant()),
        3,
    );

    let value = serde_json::to_value(patch).unwrap();

    assert_eq!(
        value,
        json!({ "id": id(), "tenant_id": tenant(), "version": 3 })
    );
}

#[test]
fn invoice_patch_requires_id_tenant_and_version() {
    assert!(
        serde_json::from_value::<InvoicePatch>(json!({ "id": id(), "tenant_id": tenant() }))
            .is_err()
    );
    assert!(serde_json::from_value::<InvoicePatch>(json!({ "id": id(), "version": 3 })).is_err());
    assert!(
        serde_json::from_value::<InvoicePatch>(json!({ "tenant_id": tenant(), "version": 3 }))
            .is_err()
    );
}

#[test]
fn invoice_row_patch_validates_set_values() {
    let valid: InvoiceRowPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "unit": "h",
    }))
    .unwrap();
    assert!(valid.validate().is_ok());
    assert_eq!(valid.unit, Patch::Set(Unit("h".to_owned())));

    let invalid_unit: InvoiceRowPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "unit": "",
    }))
    .unwrap();
    let report = invalid_unit.validate().unwrap_err();
    assert!(report.to_string().starts_with("unit"), "{report}");
}

#[test]
fn an_absent_or_cleared_nullable_value_validates() {
    let absent: InvoiceRowPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
    }))
    .unwrap();
    assert!(absent.validate().is_ok());

    let cleared: InvoiceRowPatch = serde_json::from_value(json!({
        "id": id(),
        "tenant_id": tenant(),
        "version": 1,
        "unit": null,
    }))
    .unwrap();
    assert!(cleared.validate().is_ok());
    assert_eq!(cleared.unit, Patch::Null);
}
