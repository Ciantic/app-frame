use dbkit::DbInsert;
use dbkit::DbModel;
use dbkit::DbType;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    sqlx::Type,
    DbType,
)]
#[db(pgtype = "uuid")]
#[sqlx(transparent)]
pub struct WidgetId(pub uuid::Uuid);

#[derive(DbModel, DbInsert)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: WidgetId,
}

fn main() {}
