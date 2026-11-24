use dbkit::DbDelete;
use dbkit::DbModel;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbDelete)]
#[db(table = "widget")]
struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "text")]
    name: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    version: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbDelete)]
#[db(table = "translation")]
struct Translation {
    #[db(primary_key)]
    #[db(pgtype = "text")]
    lang: String,

    #[db(primary_key)]
    #[db(pgtype = "text")]
    key: String,

    #[db(pgtype = "text")]
    value: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    version: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbDelete)]
#[db(table = "append_only")]
struct AppendOnly {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "text")]
    note: String,
}

fn id(number: u128) -> Uuid {
    Uuid::from_u128(number)
}

#[test]
fn a_versioned_key_renders_key_and_version_predicates() {
    let rendered = Widget::delete([WidgetPKV {
        id: id(1),
        version: 3,
    }])
    .render()
    .unwrap();

    assert_eq!(rendered.len(), 1);
    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"widget\" WHERE \"id\" = $1 AND \"version\" = $2"
    );
    assert_eq!(rendered[0].values, vec![json!(id(1)), json!(3)]);
}

#[test]
fn a_composite_key_keeps_declaration_order() {
    let key = TranslationPKV {
        lang: "en".to_owned(),
        key: "greeting".to_owned(),
        version: 2,
    };

    let rendered = Translation::delete([key]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"translation\" WHERE \"lang\" = $1 AND \"key\" = $2 AND \"version\" = $3"
    );
    assert_eq!(
        rendered[0].values,
        vec![json!("en"), json!("greeting"), json!(2)]
    );
}

#[test]
fn a_model_without_version_deletes_by_key_only() {
    let rendered = AppendOnly::delete([AppendOnlyPKV { id: id(1) }])
        .render()
        .unwrap();

    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"append_only\" WHERE \"id\" = $1"
    );
    assert_eq!(rendered[0].values, vec![json!(id(1))]);

    let key: AppendOnlyPK = AppendOnlyPKV { id: id(1) };
    let rendered = AppendOnly::delete([key]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"append_only\" WHERE \"id\" = $1"
    );
}

#[test]
fn every_entry_is_its_own_statement() {
    let rendered = Widget::delete([
        WidgetPKV {
            id: id(1),
            version: 0,
        },
        WidgetPKV {
            id: id(2),
            version: 4,
        },
    ])
    .render()
    .unwrap();

    assert_eq!(rendered.len(), 2);
    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"widget\" WHERE \"id\" = $1 AND \"version\" = $2"
    );
    assert_eq!(
        rendered[1].sql,
        "DELETE FROM \"widget\" WHERE \"id\" = $1 AND \"version\" = $2"
    );
    assert_eq!(rendered[0].values, vec![json!(id(1)), json!(0)]);
    assert_eq!(rendered[1].values, vec![json!(id(2)), json!(4)]);
}

#[test]
fn an_empty_batch_renders_nothing() {
    let rendered = Widget::delete([]).render().unwrap();

    assert!(rendered.is_empty());
}
