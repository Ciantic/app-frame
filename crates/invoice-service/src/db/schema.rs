use crate::db::models::MODELS;
use dbkit::schema::Schema;

pub static SCHEMA: Schema = Schema {
    name: "invoice",
    sql: include_str!("../db/schema.sql"),
    models: &MODELS,
    dependencies: &[&user_service::db::schema::SCHEMA],
};
