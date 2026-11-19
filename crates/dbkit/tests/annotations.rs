use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use uuid::Uuid;

#[derive(DbModel)]
#[db(table = "widget")]
struct Widget {
    #[db(id)]
    id: Uuid,
    name: String,
    #[db(created_at)]
    created_at: DateTime<Utc>,
    #[db(updated_at)]
    updated_at: DateTime<Utc>,
    #[db(version)]
    version: i64,
}

#[test]
fn derive_accepts_the_annotations() {
    let now = Utc::now();
    let widget = Widget {
        id: Uuid::from_u128(1),
        name: "Thingie".to_owned(),
        created_at: now,
        updated_at: now,
        version: 0,
    };

    assert_eq!(widget.id, Uuid::from_u128(1));
    assert_eq!(widget.name, "Thingie");
    assert_eq!(widget.created_at, widget.updated_at);
    assert_eq!(widget.version, 0);
}
