pub mod tenant;
pub mod user;
pub mod user_session;

use dbkit::Model;
use dbkit::TableMeta;
use std::sync::LazyLock;

pub static MODELS: LazyLock<Vec<TableMeta>> = LazyLock::new(|| {
    vec![
        user::User::meta().into_table(),
        user_session::UserSession::meta().into_table(),
        tenant::Tenant::meta().into_table(),
    ]
});
