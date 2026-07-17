use notion_ical_sync::{AppState, PageInfo, create_app};
use tokio::net::TcpListener;

#[tokio::test]
async fn test_caldav_server_operations() {
    // 1. Create a mocked AppState with pre-populated cache
    let db_id = "test-db-12345".to_string();
    let state = AppState::new(
        "mock-notion-token".to_string(),
        vec![db_id.clone()],
        vec!["mock-ds-id".to_string()],
        "Date".to_string(),
    );

    // Seed mock event
    let event_id = "event-abc-98765".to_string();
    let initial_event = PageInfo {
        id: event_id.clone(),
        title: "Initial Sync Event".to_string(),
        start: "2026-07-18T10:00:00Z".to_string(),
        end: Some("2026-07-18T11:00:00Z".to_string()),
        url: "https://notion.so/event-abc-98765".to_string(),
        last_edited: "2026-07-18T00:00:00Z".to_string(),
    };
    {
        let mut cache = state.cache.write().await;
        cache.insert(db_id.clone(), vec![initial_event]);
    }

    // 2. Start the router on a random port
    let app = create_app(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 3. Test PROPFIND /cal/{db_id} (Depth: 0)
    let propfind_res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &format!("{}/cal/{}", base_url, db_id))
        .header("Depth", "0")
        .send()
        .await
        .unwrap();
    assert_eq!(propfind_res.status().as_u16(), 207); // Multi-Status
    let propfind_body = propfind_res.text().await.unwrap();
    assert!(propfind_body.contains("test-db-12345"));
    assert!(propfind_body.contains("<C:calendar/>"));

    // 4. Test PROPFIND /cal/{db_id} (Depth: 1)
    let propfind_depth1_res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &format!("{}/cal/{}", base_url, db_id))
        .header("Depth", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(propfind_depth1_res.status().as_u16(), 207);
    let propfind_depth1_body = propfind_depth1_res.text().await.unwrap();
    assert!(propfind_depth1_body.contains("eventabc98765.ics"));

    // 5. Test REPORT /cal/{db_id}
    let report_res = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), &format!("{}/cal/{}", base_url, db_id))
        .send()
        .await
        .unwrap();
    assert_eq!(report_res.status().as_u16(), 207);
    let report_body = report_res.text().await.unwrap();
    assert!(report_body.contains("Initial Sync Event"));
    assert!(report_body.contains("BEGIN:VCALENDAR"));

    // 6. Test GET /cal/{db_id}/{event_id}.ics
    let get_res = client
        .get(&format!("{}/cal/{}/{}.ics", base_url, db_id, event_id))
        .send()
        .await
        .unwrap();
    assert_eq!(get_res.status(), 200);
    let get_body = get_res.text().await.unwrap();
    assert!(get_body.contains("SUMMARY:Initial Sync Event"));

    // 7. Test PUT /cal/{db_id}/{new_event_id}.ics (create new event)
    let new_event_id = "new-event-777".to_string();
    let new_ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:new-event-777
SUMMARY:Created via PUT
DTSTART:20260718T150000Z
DTEND:20260718T160000Z
DESCRIPTION:Put Description
END:VEVENT
END:VCALENDAR"#;

    let put_res = client
        .put(&format!("{}/cal/{}/{}.ics", base_url, db_id, new_event_id))
        .body(new_ics)
        .send()
        .await
        .unwrap();
    assert!(put_res.status() == 201 || put_res.status() == 204);

    // Check if GET returns the new event
    let get_new_res = client
        .get(&format!("{}/cal/{}/{}.ics", base_url, db_id, new_event_id))
        .send()
        .await
        .unwrap();
    assert_eq!(get_new_res.status(), 200);
    let get_new_body = get_new_res.text().await.unwrap();
    assert!(get_new_body.contains("SUMMARY:Created via PUT"));
    assert!(get_new_body.contains("DTSTART:20260718T150000Z"));

    // 8. Test DELETE /cal/{db_id}/{new_event_id}.ics
    let delete_res = client
        .delete(&format!("{}/cal/{}/{}.ics", base_url, db_id, new_event_id))
        .send()
        .await
        .unwrap();
    assert_eq!(delete_res.status(), 204);

    // Verify it is gone
    let get_deleted_res = client
        .get(&format!("{}/cal/{}/{}.ics", base_url, db_id, new_event_id))
        .send()
        .await
        .unwrap();
    assert_eq!(get_deleted_res.status(), 404);
}
