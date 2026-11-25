use crate::db::models::MODELS;
use dbkit::schema::Schema;
use dbkit::schema::SchemaSource;

pub static SCHEMA: Schema = Schema {
    name: "email",
    sql: include_str!("../db/schema.sql"),
    models: &MODELS,
    dependencies: &[],
    source: SchemaSource::Handwritten,
};
