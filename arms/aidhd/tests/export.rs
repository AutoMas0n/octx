use octx_aidhd::db::{add_task, open_in_memory};
use octx_aidhd::export::{export_csv, export_json};
use octx_aidhd::model::{NewTask, Priority};

#[test]
fn json_export_round_trips_tasks() {
    let mut conn = open_in_memory().unwrap();
    add_task(
        &mut conn,
        &NewTask {
            title: "fix garage".into(),
            priority: Some(Priority::High),
            tags: vec!["home".into()],
            ..NewTask::default()
        },
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.json");
    export_json(&conn, Some(&path)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let tasks: Vec<octx_aidhd::model::Task> = serde_json::from_str(&text).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title, "fix garage");
    assert_eq!(tasks[0].tags, vec!["home".to_string()]);
}

#[test]
fn csv_export_writes_header_and_rows() {
    let mut conn = open_in_memory().unwrap();
    add_task(
        &mut conn,
        &NewTask {
            title: "buy milk, eggs".into(),
            ..NewTask::default()
        },
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.csv");
    export_csv(&conn, Some(&path)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines = text.lines();
    assert!(lines.next().unwrap().starts_with("id,title,description"));
    let row = lines.next().unwrap();
    assert!(row.contains("\"buy milk, eggs\""));
}
