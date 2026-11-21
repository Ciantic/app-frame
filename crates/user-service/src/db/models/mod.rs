pub mod user;
pub mod user_session;

use dbkit::Model;
use dbkit::TableMeta;

pub const MODELS: &[TableMeta] = &[user::User::META, user_session::UserSession::META];
