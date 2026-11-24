// Shared model, included by each case.
use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use uuid::Uuid;

#[derive(DbModel)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
#[db(table = "orders")]
pub struct Order {
    #[db(primary_key, pgtype = "uuid")]
    pub id: Uuid,

    #[db(pgtype = "varchar(20)")]
    #[db(query_ops(eq, in))]
    pub status: String,

    #[db(pgtype = "int8")]
    #[db(query_ops(gte, lt))]
    pub total: i64,

    #[db(pgtype = "bool")]
    #[db(query_ops(eq))]
    pub vip: bool,

    #[db(pgtype = "text[]")]
    #[db(query_ops(contains))]
    pub tags: Vec<String>,

    #[db(pgtype = "timestamptz")]
    pub created_at: DateTime<Utc>,
}
