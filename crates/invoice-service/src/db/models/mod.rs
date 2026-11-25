pub mod customer;
pub mod invoice;
pub mod invoice_row;

use dbkit::Model;
use dbkit::TableMeta;
use std::sync::LazyLock;

pub static MODELS: LazyLock<Vec<TableMeta>> = LazyLock::new(|| {
    vec![
        customer::Customer::meta().into_table(),
        invoice::Invoice::meta().into_table(),
        invoice_row::InvoiceRow::meta().into_table(),
    ]
});
