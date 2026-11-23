pub mod email;

use dbkit::Model;
use dbkit::TableMeta;
use std::sync::LazyLock;

pub static MODELS: LazyLock<Vec<TableMeta>> =
    LazyLock::new(|| vec![email::Email::meta().into_table()]);
