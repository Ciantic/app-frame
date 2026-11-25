use crate::db::models::MODELS;
use dbkit::schema::Schema;

pub static SCHEMA: Schema = Schema {
    name: "user",
    sql: include_str!("../db/schema.sql"),
    models: &MODELS,
    dependencies: &[],
};
