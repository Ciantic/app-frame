use chrono::DateTime;
use chrono::Utc;
use dbkit_macros::DbModel;
use uuid::Uuid;

#[derive(DbModel)]
#[db(table = "widget")]
struct Widget {
    #[db(primary_key)]
    id: Uuid,
    name: String,
    #[db(created_at)]
    created_at: DateTime<Utc>,
    #[db(updated_at)]
    updated_at: DateTime<Utc>,
    #[db(version)]
    version: i64,
}

#[derive(DbModel)]
#[db(table = "membership")]
struct Membership {
    #[db(primary_key)]
    user_id: Uuid,
    #[db(primary_key)]
    team_id: Uuid,
    #[db(version)]
    version: i64,
}

/// Proves the derive runs on a model shape and leaves the struct usable.
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

/// Proves a composite primary key needs no extra ceremony beyond repeating the
/// role.
#[test]
fn derive_accepts_a_composite_primary_key() {
    let membership = Membership {
        user_id: Uuid::from_u128(1),
        team_id: Uuid::from_u128(2),
        version: 0,
    };

    assert_eq!(membership.user_id, Uuid::from_u128(1));
    assert_eq!(membership.team_id, Uuid::from_u128(2));
    assert_eq!(membership.version, 0);
}
