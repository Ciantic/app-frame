use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::Model;
use dbkit::TableMeta;
use uuid::Uuid;

#[derive(DbModel)]
#[db(table = "widget")]
struct Widget {
    #[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]
    id: Uuid,
    #[db(pgtype = "text")]
    name: String,
    #[db(created_at, pgtype = "timestamptz", pgdefault = "now()")]
    created_at: DateTime<Utc>,
    #[db(updated_at, pgtype = "timestamptz", pgdefault = "now()")]
    updated_at: DateTime<Utc>,
    #[db(version, pgtype = "int8", pgdefault = "0")]
    version: i64,
}

#[derive(DbModel)]
#[db(table = "membership")]
struct Membership {
    #[db(primary_key, pgtype = "uuid")]
    user_id: Uuid,
    #[db(primary_key, pgtype = "uuid")]
    team_id: Uuid,
}

/// Proves a model without a version column derives and stays usable.
#[derive(DbModel)]
#[db(table = "widget_log")]
struct WidgetLog {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(pgtype = "text")]
    message: String,
    #[db(pgtype = "text")]
    note: Option<String>,
}

const MODELS: &[TableMeta] = &[Widget::META, WidgetLog::META];

#[test]
fn derive_generates_table_metadata() {
    let meta = Widget::META;

    assert_eq!(meta.table, "widget");
    assert_eq!(meta.primary_key, ["id"]);
    assert_eq!(meta.version, Some("version"));
    assert_eq!(meta.created_at, Some("created_at"));
    assert_eq!(meta.updated_at, Some("updated_at"));

    let columns: Vec<(&str, bool, &str, Option<&str>)> = meta
        .columns
        .iter()
        .map(|column| {
            (
                column.name,
                column.nullable,
                column.pg_type,
                column.pg_default,
            )
        })
        .collect();

    assert_eq!(
        columns,
        [
            ("id", false, "uuid", Some("uuidv7()")),
            ("name", false, "text", None),
            ("created_at", false, "timestamptz", Some("now()")),
            ("updated_at", false, "timestamptz", Some("now()")),
            ("version", false, "int8", Some("0")),
        ]
    );
}

#[test]
fn derive_omits_absent_roles_and_marks_optionals() {
    let meta = <WidgetLog as Model>::META;

    assert_eq!(meta.table, "widget_log");
    assert_eq!(meta.version, None);
    assert_eq!(meta.created_at, None);
    assert_eq!(meta.updated_at, None);
    assert!(
        meta.columns
            .iter()
            .any(|column| column.name == "note" && column.nullable)
    );
}

#[test]
fn models_lists_every_model() {
    let tables: Vec<&str> = MODELS.iter().map(|meta| meta.table).collect();

    assert_eq!(tables, ["widget", "widget_log"]);
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
    };

    assert_eq!(membership.user_id, Uuid::from_u128(1));
    assert_eq!(membership.team_id, Uuid::from_u128(2));
}

/// Proves a model with no version column derives.
#[test]
fn derive_accepts_a_model_without_version() {
    let log = WidgetLog {
        id: Uuid::from_u128(1),
        message: "created".to_owned(),
        note: None,
    };

    assert_eq!(log.id, Uuid::from_u128(1));
    assert_eq!(log.message, "created");
    assert!(log.note.is_none());
}
