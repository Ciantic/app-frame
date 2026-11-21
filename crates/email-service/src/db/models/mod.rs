pub mod email;

use dbkit::Model;
use dbkit::TableMeta;

pub const MODELS: &[TableMeta] = &[email::Email::META];
