use notion_ical_sync::{auth, AppState, CaldavAllowWrites, PageInfo, create_app};
use axum::Router;
use tokio::net::TcpListener;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Fixed plaintext password every seeded test calendar shares — real per-row
/// argon2 hashes (see `test_state_multi`) are what actually gets verified,
/// this constant just needs to match what was hashed at seed time.
const TEST_CALDAV_PASSWORD: &str = "test-caldav-pass-123";

fn caldav_username(db_id: &str) -> String {
    format!("caldav-{}", db_id)
}

fn basic_auth_header(db_id: &str) -> String {
    format!(
        "Basic {}",
        base64_light::base64_encode(&format!("{}:{}", caldav_username(db_id), TEST_CALDAV_PASSWORD))
    )
}

fn test_caldav_password_hash() -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};
    use argon2::Argon2;
    let salt = SaltString::generate(&mut rand::thread_rng());
    Argon2::default().hash_password(TEST_CALDAV_PASSWORD.as_bytes(), &salt).unwrap().to_string()
}

/// create_app now also wires the SaaS's own Keycloak login, which needs a
/// real OidcClient (built by discovering a live issuer). These CalDAV
/// protocol tests don't exercise login at all, but still have to hand one
/// in — point it at the same local dev Keycloak realm Terraform provisioned
/// (`terraform apply` in terraform/, against the docker-compose Keycloak).
async fn test_create_app(state: AppState) -> Router {
    let issuer = std::env::var("TEST_KEYCLOAK_ISSUER_URL")
        .unwrap_or_else(|_| "http://localhost:8081/realms/notion-caldav-saas".to_string());
    // The realm's client is confidential (see terraform/main.tf) — Keycloak
    // rejects unauthenticated requests from it without this, which showed up
    // as a 400 (Keycloak's own error page body) on *every* request once
    // oidc_auth_service tried to use the client, not just login attempts.
    let client_secret = std::env::var("TEST_KEYCLOAK_CLIENT_SECRET").ok();
    let oidc_client = auth::build_oidc_client(
        issuer,
        "notion-caldav-saas-app".to_string(),
        client_secret,
        "http://localhost:0/oidc".to_string(),
    )
    .await;
    let app_config = auth::AppConfig { base_url: "http://localhost:0".to_string() };
    // oidc_auth_service (applied inside create_app, wrapping every route
    // including the plain CalDAV/Basic-Auth ones) needs a live
    // tower_sessions::Session to be extractable on every request, or it
    // 500s instead of passing through — a real session store, not just
    // required for actual login flows.
    let session_layer = tower_sessions::SessionManagerLayer::new(tower_sessions::MemoryStore::default());
    create_app(state, oidc_client, app_config).layer(session_layer)
}

/// Builds a real DB-backed AppState against a test Postgres, seeding a
/// user/notion_connection/calendar row so DB-backed lookups (calendar_by_db_id
/// etc.) resolve `db_id`/`ds_id` the same way the pre-multi-tenant AppState's
/// flat notion_token/database_ids fields used to provide directly. Each
/// calendar also gets a real argon2-hashed CalDAV password (see
/// `TEST_CALDAV_PASSWORD`/`basic_auth_header`) since Basic Auth is now
/// DB-backed and unconditionally required — there's no more "auth disabled"
/// env-var bypass to lean on. Event data itself is still seeded straight
/// into `state.cache` by each test, same as before — only calendar
/// *identity* comes from Postgres.
async fn test_state(db_id: &str, ds_id: &str, allow_writes: CaldavAllowWrites) -> AppState {
    test_state_multi(&[(db_id, ds_id)], allow_writes).await
}

async fn test_state_multi(calendars: &[(&str, &str)], allow_writes: CaldavAllowWrites) -> AppState {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://biolink:biolink@localhost:5433/notion_saas".to_string());
    let pool = sqlx::PgPool::connect(&db_url).await.expect("connect to test db");
    sqlx::migrate!("./migrations").run(&pool).await.expect("run migrations");

    let password_hash = test_caldav_password_hash();

    for (db_id, ds_id) in calendars {
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (keycloak_sub, email) VALUES ($1, '')
             ON CONFLICT (keycloak_sub) DO UPDATE SET email = users.email
             RETURNING id",
        )
        .bind(format!("test-sub-{}", db_id))
        .fetch_one(&pool)
        .await
        .unwrap();

        let conn_id: i64 = sqlx::query_scalar(
            "INSERT INTO notion_connections (user_id, notion_access_token, workspace_id)
             VALUES ($1, 'mock-notion-token', 'mock-workspace')
             ON CONFLICT (user_id, workspace_id) DO UPDATE SET notion_access_token = EXCLUDED.notion_access_token
             RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO calendars (user_id, notion_connection_id, database_id, data_source_id, date_property, caldav_username, caldav_password_hash)
             VALUES ($1, $2, $3, $4, 'Date', $5, $6)
             ON CONFLICT (database_id) DO UPDATE SET
                data_source_id = EXCLUDED.data_source_id,
                notion_connection_id = EXCLUDED.notion_connection_id,
                user_id = EXCLUDED.user_id,
                caldav_username = EXCLUDED.caldav_username,
                caldav_password_hash = EXCLUDED.caldav_password_hash",
        )
        .bind(user_id)
        .bind(conn_id)
        .bind(*db_id)
        .bind(*ds_id)
        .bind(caldav_username(db_id))
        .bind(&password_hash)
        .execute(&pool)
        .await
        .unwrap();
    }

    AppState::new(pool, allow_writes, None, None)
}

#[tokio::test]
async fn test_caldav_server_operations() {
    let _lock = TEST_MUTEX.lock().unwrap();
    // 1. Create a mocked AppState with pre-populated cache
    let db_id = "test-db-12345".to_string();
    let state = test_state(&db_id, "mock-ds-id", CaldavAllowWrites::True).await;
    let auth_header = basic_auth_header(&db_id);

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
    let app = test_create_app(state).await;
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
        .header("Authorization", &auth_header)
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
        .header("Authorization", &auth_header)
        .send()
        .await
        .unwrap();
    assert_eq!(propfind_depth1_res.status().as_u16(), 207);
    let propfind_depth1_body = propfind_depth1_res.text().await.unwrap();
    assert!(propfind_depth1_body.contains("eventabc98765.ics"));

    // 5. Test REPORT /cal/{db_id}
    let report_res = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), &format!("{}/cal/{}", base_url, db_id))
        .header("Authorization", &auth_header)
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
        .header("Authorization", &auth_header)
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
        .header("Authorization", &auth_header)
        .body(new_ics)
        .send()
        .await
        .unwrap();
    assert!(put_res.status() == 201 || put_res.status() == 204);

    // Check if GET returns the new event
    let get_new_res = client
        .get(&format!("{}/cal/{}/{}.ics", base_url, db_id, new_event_id))
        .header("Authorization", &auth_header)
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
        .header("Authorization", &auth_header)
        .send()
        .await
        .unwrap();
    assert_eq!(delete_res.status(), 204);

    // Verify it is gone
    let get_deleted_res = client
        .get(&format!("{}/cal/{}/{}.ics", base_url, db_id, new_event_id))
        .header("Authorization", &auth_header)
        .send()
        .await
        .unwrap();
    assert_eq!(get_deleted_res.status(), 404);
}

#[tokio::test]
async fn test_caldav_host_based_routing() {
    let _lock = TEST_MUTEX.lock().unwrap();
    // 1. Create a mocked AppState with pre-populated cache for both databases
    let db_id_cal = "4cb38c7656ae483d8ee5650d9fb02108".to_string();
    let db_id_time = "39e6a94a90a680da85d2c29e3c52ed8e".to_string();

    let state = test_state_multi(
        &[(&db_id_cal, "mock-ds-1"), (&db_id_time, "mock-ds-2")],
        CaldavAllowWrites::True,
    )
    .await;
    let auth_cal = basic_auth_header(&db_id_cal);
    let auth_time = basic_auth_header(&db_id_time);

    // Seed mock event for calendar.opendiy.vn
    let event_id_cal = "event-cal-111".to_string();
    let event_cal = PageInfo {
        id: event_id_cal.clone(),
        title: "Calendar Event".to_string(),
        start: "2026-07-18T10:00:00Z".to_string(),
        end: Some("2026-07-18T11:00:00Z".to_string()),
        url: "https://notion.so/event-cal-111".to_string(),
        last_edited: "2026-07-18T00:00:00Z".to_string(),
    };

    // Seed mock event for mytime.opendiy.vn
    let event_id_time = "event-time-222".to_string();
    let event_time = PageInfo {
        id: event_id_time.clone(),
        title: "Time Event".to_string(),
        start: "2026-07-18T12:00:00Z".to_string(),
        end: Some("2026-07-18T13:00:00Z".to_string()),
        url: "https://notion.so/event-time-222".to_string(),
        last_edited: "2026-07-18T00:00:00Z".to_string(),
    };

    {
        let mut cache = state.cache.write().await;
        cache.insert(db_id_cal.clone(), vec![event_cal]);
        cache.insert(db_id_time.clone(), vec![event_time]);
    }

    // 2. Start the router on a random port
    let app = test_create_app(state).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 3. Test calendar.opendiy.vn PROPFIND / (Depth: 0)
    let res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &base_url)
        .header("Host", "calendar.opendiy.vn")
        .header("Depth", "0")
        .header("Authorization", &auth_cal)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 207);
    let body = res.text().await.unwrap();
    assert!(body.contains("<D:href>/</D:href>"));

    // 4. Test calendar.opendiy.vn PROPFIND / (Depth: 1)
    let res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &base_url)
        .header("Host", "calendar.opendiy.vn")
        .header("Depth", "1")
        .header("Authorization", &auth_cal)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 207);
    let body = res.text().await.unwrap();
    assert!(body.contains("<D:href>/eventcal111.ics</D:href>"));

    // 5. Test calendar.opendiy.vn GET /eventcal111.ics
    let res = client
        .get(&format!("{}/eventcal111.ics", base_url))
        .header("Host", "calendar.opendiy.vn")
        .header("Authorization", &auth_cal)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("SUMMARY:Calendar Event"));

    // 6. Test mytime.opendiy.vn PROPFIND / (Depth: 1)
    let res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &base_url)
        .header("Host", "mytime.opendiy.vn")
        .header("Depth", "1")
        .header("Authorization", &auth_time)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 207);
    let body = res.text().await.unwrap();
    assert!(body.contains("<D:href>/eventtime222.ics</D:href>"));

    // 7. Test mytime.opendiy.vn GET /eventtime222.ics
    let res = client
        .get(&format!("{}/eventtime222.ics", base_url))
        .header("Host", "mytime.opendiy.vn")
        .header("Authorization", &auth_time)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("SUMMARY:Time Event"));

    // 7b. Cross-calendar credentials must be rejected even on a route that
    // *does* successfully resolve a host-mapped db_id — this is the actual
    // point of Phase 4's DB-backed auth: a valid credential for one
    // calendar must not open a different one just because both exist.
    let res = client
        .get(&format!("{}/eventtime222.ics", base_url))
        .header("Host", "mytime.opendiy.vn")
        .header("Authorization", &auth_cal)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);

    // 8. Test fallback path-based routing on calendar.opendiy.vn or localhost
    let res = client
        .get(&format!("{}/cal/{}/eventcal111.ics", base_url, db_id_cal))
        .header("Authorization", &auth_cal)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("SUMMARY:Calendar Event"));

    // 9. Test unmapped host (should return 404, given valid credentials —
    // otherwise auth itself would reject before the handler even runs)
    let res = client
        .get(&format!("{}/eventcal111.ics", base_url))
        .header("Host", "other.opendiy.vn")
        .header("Authorization", &auth_cal)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn test_caldav_new_endpoints_and_auth() {
    let _lock = TEST_MUTEX.lock().unwrap();

    let db_id = "4cb38c7656ae483d8ee5650d9fb02108".to_string();
    let state = test_state(&db_id, "mock-ds-id", CaldavAllowWrites::True).await;
    let username = caldav_username(&db_id);
    let auth_header_val = basic_auth_header(&db_id);

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

    let app = test_create_app(state).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 1. Test unauthorized request
    let unauth_res = client
        .put(&format!("{}/cal/{}/unauth-event.ics", base_url, db_id))
        .body("BEGIN:VCALENDAR\nEND:VCALENDAR")
        .send()
        .await
        .unwrap();
    assert_eq!(unauth_res.status(), 401);
    assert_eq!(unauth_res.headers().get("WWW-Authenticate").unwrap().to_str().unwrap(), "Basic realm=\"CalDAV Server\"");

    // 1b. Test wrong-password request (real credential lookup, not just "any header present")
    let bad_auth = format!("Basic {}", base64_light::base64_encode(&format!("{}:wrong-password", username)));
    let bad_res = client
        .put(&format!("{}/cal/{}/unauth-event.ics", base_url, db_id))
        .header("Authorization", &bad_auth)
        .body("BEGIN:VCALENDAR\nEND:VCALENDAR")
        .send()
        .await
        .unwrap();
    assert_eq!(bad_res.status(), 401);

    // 2. Test authorized request to well-known (with redirect)
    // Test direct redirect by turning off auto-redirects
    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let redirect_res = no_redirect_client
        .get(&format!("{}/.well-known/caldav", base_url))
        .header("Authorization", &auth_header_val)
        .send()
        .await
        .unwrap();
    assert_eq!(redirect_res.status(), 301);
    assert_eq!(redirect_res.headers().get("Location").unwrap().to_str().unwrap(), "/principals/");
    assert_eq!(redirect_res.headers().get("dav").unwrap().to_str().unwrap(), "1, 3, calendar-access");

    // 3. Test PROPFIND /principals/
    let propfind_princ_res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &format!("{}/principals/", base_url))
        .header("Authorization", &auth_header_val)
        .send()
        .await
        .unwrap();
    assert_eq!(propfind_princ_res.status(), 207);
    let princ_body = propfind_princ_res.text().await.unwrap();
    assert!(princ_body.contains("<D:current-user-principal>"));
    assert!(princ_body.contains("<C:calendar-home-set>"));
    assert!(princ_body.contains(&format!("/calendars/{}/", username)));

    // 4. Test PROPFIND /calendars/{username}/
    let propfind_cal_res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &format!("{}/calendars/{}/", base_url, username))
        .header("Authorization", &auth_header_val)
        .send()
        .await
        .unwrap();
    assert_eq!(propfind_cal_res.status(), 207);
    let cal_body = propfind_cal_res.text().await.unwrap();
    assert!(cal_body.contains("<D:displayname>"));
    assert!(cal_body.contains("<C:calendar/>"));
    assert!(cal_body.contains("<C:comp name=\"VEVENT\"/>"));

    // 5. Test PROPFIND / (Root probe, unmapped host)
    let root_propfind_res = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &format!("{}/", base_url))
        .header("Authorization", &auth_header_val)
        .send()
        .await
        .unwrap();
    assert_eq!(root_propfind_res.status(), 207);
    let root_propfind_body = root_propfind_res.text().await.unwrap();
    assert!(root_propfind_body.contains("<D:href>/</D:href>"));
    assert!(root_propfind_body.contains("<D:current-user-principal>"));

    // 6. Test REPORT /calendars/{username}/
    let report_cal_res = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), &format!("{}/calendars/{}/", base_url, username))
        .header("Authorization", &auth_header_val)
        .send()
        .await
        .unwrap();
    assert_eq!(report_cal_res.status(), 207);
    let report_cal_body = report_cal_res.text().await.unwrap();
    assert!(report_cal_body.contains("BEGIN:VCALENDAR"));

    // 7. Test OPTIONS / (Auth bypass)
    let options_res = client
        .request(reqwest::Method::OPTIONS, &format!("{}/", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(options_res.status(), 200);
    assert!(options_res.headers().contains_key("dav"));
}

#[tokio::test]
async fn test_caldav_readonly_mode() {
    let _lock = TEST_MUTEX.lock().unwrap();

    let db_id = "test-db-readonly".to_string();
    let state = test_state(&db_id, "mock-ds-id", CaldavAllowWrites::False).await;
    let auth_header = basic_auth_header(&db_id);

    let app = test_create_app(state).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 1. Verify health endpoint returns caldav_allow_writes: "false"
    let health_res = client
        .get(&format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(health_res.status(), 200);
    let health_json: serde_json::Value = health_res.json().await.unwrap();
    assert_eq!(health_json["caldav_allow_writes"], "false");

    // 2. Test PUT on event -> 403 Forbidden (reached only once real
    // credentials pass auth — CaldavAllowWrites::False is checked inside the
    // handler, after the DB-backed Basic Auth middleware).
    let put_res = client
        .put(&format!("{}/cal/{}/event123.ics", base_url, db_id))
        .header("Authorization", &auth_header)
        .body("BEGIN:VCALENDAR\nEND:VCALENDAR")
        .send()
        .await
        .unwrap();
    assert_eq!(put_res.status(), 403);

    // 3. Test DELETE on event -> 403 Forbidden
    let delete_res = client
        .delete(&format!("{}/cal/{}/event123.ics", base_url, db_id))
        .header("Authorization", &auth_header)
        .send()
        .await
        .unwrap();
    assert_eq!(delete_res.status(), 403);

    // 4. Test PROPPATCH on collection -> 403 Forbidden
    let proppatch_res = client
        .request(
            reqwest::Method::from_bytes(b"PROPPATCH").unwrap(),
            &format!("{}/cal/{}", base_url, db_id),
        )
        .header("Authorization", &auth_header)
        .body("<xml></xml>")
        .send()
        .await
        .unwrap();
    assert_eq!(proppatch_res.status(), 403);
}

#[tokio::test]
async fn test_app_webview_requires_oidc_not_basic_auth() {
    let _lock = TEST_MUTEX.lock().unwrap();

    let db_id = "test-db-webview".to_string();
    let state = test_state(&db_id, "mock-ds-id", CaldavAllowWrites::True).await;
    let caldav_auth = basic_auth_header(&db_id);

    let app = test_create_app(state).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let base_url = format!("http://{}", addr);

    // /app moved from CalDAV Basic Auth to OIDC session auth in Phase 4 — a
    // valid CalDAV credential must NOT be enough to open the webview
    // (they're different identities, see auth.rs vs oauth.rs), and no
    // session at all should force a login redirect rather than a CalDAV 401.
    let res = no_redirect_client
        .get(&format!("{}/app", base_url))
        .header("Authorization", &caldav_auth)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 303);
    let location = res.headers().get("Location").unwrap().to_str().unwrap();
    assert!(location.contains("/realms/notion-caldav-saas/protocol/openid-connect/auth"));
}
