use axum::{
    extract::Query,
    response::{Html, IntoResponse},
};
use leptos::prelude::*;
use leptos::tachys::view::RenderHtml;
use serde::{Deserialize, Serialize};
use icalendar::{Calendar, Component};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEvent {
    pub summary: String,
    pub start: String,
    pub end: String,
    pub description: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct UiParams {
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub calendar_path: Option<String>,
}

// Helper to find <calendar-data> tags in multi-status response
fn find_calendar_data_start(s: &str) -> Option<usize> {
    let mut cursor = 0;
    while let Some(idx) = s[cursor..].find("<") {
        let start = cursor + idx;
        let rest = &s[start..];
        if let Some(end_tag_idx) = rest.find(">") {
            let tag_content = &rest[1..end_tag_idx];
            let tag_name = tag_content.split_whitespace().next().unwrap_or("");
            if tag_name == "calendar-data" || tag_name.ends_with(":calendar-data") {
                return Some(start + end_tag_idx + 1);
            }
            cursor = start + 1;
        } else {
            break;
        }
    }
    None
}

fn find_calendar_data_end(s: &str) -> Option<usize> {
    let mut cursor = 0;
    while let Some(idx) = s[cursor..].find("</") {
        let start = cursor + idx;
        let rest = &s[start..];
        if let Some(end_tag_idx) = rest.find(">") {
            let tag_content = rest[2..end_tag_idx].trim();
            if tag_content == "calendar-data" || tag_content.ends_with(":calendar-data") {
                return Some(start);
            }
            cursor = start + 2;
        } else {
            break;
        }
    }
    None
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}

fn format_date_perhaps_time(dt: &icalendar::DatePerhapsTime) -> String {
    match dt {
        icalendar::DatePerhapsTime::Date(d) => d.format("%Y-%m-%d").to_string(),
        icalendar::DatePerhapsTime::DateTime(cdt) => match cdt {
            icalendar::CalendarDateTime::Utc(dt) => dt.to_rfc3339(),
            icalendar::CalendarDateTime::Floating(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            icalendar::CalendarDateTime::WithTimezone { date_time, tzid } => {
                format!("{} ({})", date_time.format("%Y-%m-%d %H:%M:%S"), tzid)
            }
        }
    }
}

async fn fetch_caldav_events(
    server_url: &str,
    username: &str,
    password: &str,
    calendar_path: &str,
) -> Result<Vec<UiEvent>, String> {
    let base_url = server_url.trim_end_matches('/');
    let path = calendar_path.trim_start_matches('/');
    let full_url = format!("{}/{}", base_url, path);

    let client = reqwest::Client::new();
    let xml_body = r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
        <d:prop>
            <d:getetag />
            <c:calendar-data />
        </d:prop>
        <c:filter>
            <c:comp-filter name="VCALENDAR" />
        </c:filter>
    </c:calendar-query>"#;

    let mut request = client.request(
        reqwest::Method::from_bytes(b"REPORT").map_err(|e| e.to_string())?,
        &full_url,
    )
    .header("Content-Type", "application/xml; charset=utf-8")
    .header("Depth", "1")
    .body(xml_body);

    if !username.is_empty() {
        request = request.basic_auth(username, Some(password));
    }

    let mut ui_events = Vec::new();
    let mut fallback_to_get = true;

    if let Ok(response) = request.send().await {
        let status = response.status();
        if status.is_success() || status == reqwest::StatusCode::MULTI_STATUS {
            if let Ok(body_text) = response.text().await {
                let mut parsed_events = Vec::new();
                let mut search_str = &body_text[..];
                while let Some(start_idx) = find_calendar_data_start(search_str) {
                    let content_start = start_idx;
                    if let Some(end_idx) = find_calendar_data_end(&search_str[content_start..]) {
                        let calendar_data = &search_str[content_start..content_start + end_idx];
                        let unescaped = unescape_xml(calendar_data);

                        if let Ok(calendar) = Calendar::from_str(&unescaped) {
                            for event in calendar.events() {
                                let summary = event.get_summary().unwrap_or("No Title").to_string();
                                let start = event.get_start().map(|t| format_date_perhaps_time(&t)).unwrap_or_else(|| "N/A".to_string());
                                let end = event.get_end().map(|t| format_date_perhaps_time(&t)).unwrap_or_else(|| "N/A".to_string());
                                let description = event.get_description().unwrap_or("").to_string();

                                parsed_events.push(UiEvent {
                                    summary,
                                    start,
                                    end,
                                    description,
                                });
                            }
                        }
                        search_str = &search_str[content_start + end_idx..];
                    } else {
                        break;
                    }
                }
                if !parsed_events.is_empty() {
                    ui_events = parsed_events;
                    fallback_to_get = false;
                }
            }
        }
    }

    if fallback_to_get {
        let mut get_request = client.get(&full_url);
        if !username.is_empty() {
            get_request = get_request.basic_auth(username, Some(password));
        }
        let get_response = get_request.send().await.map_err(|e| e.to_string())?;
        let get_status = get_response.status();
        if !get_status.is_success() {
            return Err(format!("Server returned HTTP status {} on GET fallback", get_status));
        }
        let get_body = get_response.text().await.map_err(|e| e.to_string())?;
        if let Ok(calendar) = Calendar::from_str(&get_body) {
            for event in calendar.events() {
                let summary = event.get_summary().unwrap_or("No Title").to_string();
                let start = event.get_start().map(|t| format_date_perhaps_time(&t)).unwrap_or_else(|| "N/A".to_string());
                let end = event.get_end().map(|t| format_date_perhaps_time(&t)).unwrap_or_else(|| "N/A".to_string());
                let description = event.get_description().unwrap_or("").to_string();

                ui_events.push(UiEvent {
                    summary,
                    start,
                    end,
                    description,
                });
            }
        } else {
            return Err("Failed to parse ICS calendar from GET response".to_string());
        }
    }

    Ok(ui_events)
}

#[component]
pub fn CalDavUi(
    server_url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    calendar_path: Option<String>,
    error: Option<String>,
    events: Option<Vec<UiEvent>>,
) -> impl IntoView {
    let s_url = server_url.unwrap_or_else(|| "http://localhost:8080".to_string());
    let u_name = username.unwrap_or_default();
    let p_word = password.unwrap_or_default();
    let c_path = calendar_path.unwrap_or_default();

    view! {
        <html lang="en">
        <head>
            <meta charset="UTF-8" />
            <meta name="viewport" content="width=device-width, initial-scale=1.0" />
            <title>"CalDAV Testing UI"</title>
            <style>
                r#"
                body {
                    margin: 0;
                    font-family: 'Outfit', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
                    background: linear-gradient(135deg, #0f172a 0%, #1e1b4b 100%);
                    color: #f8fafc;
                    min-height: 100vh;
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    padding: 2rem 1rem;
                    box-sizing: border-box;
                }
                .container {
                    width: 100%;
                    max-width: 900px;
                }
                header {
                    text-align: center;
                    margin-bottom: 2.5rem;
                }
                h1 {
                    font-size: 2.5rem;
                    font-weight: 800;
                    background: linear-gradient(to right, #6366f1, #a855f7);
                    -webkit-background-clip: text;
                    -webkit-text-fill-color: transparent;
                    margin: 0 0 0.5rem 0;
                    letter-spacing: -0.025em;
                }
                .subtitle {
                    color: #94a3b8;
                    font-size: 1.1rem;
                    margin: 0;
                }
                .card {
                    background: rgba(30, 41, 59, 0.7);
                    backdrop-filter: blur(12px);
                    -webkit-backdrop-filter: blur(12px);
                    border: 1px solid rgba(99, 102, 241, 0.2);
                    border-radius: 16px;
                    padding: 2rem;
                    box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.3), 0 8px 10px -6px rgba(0, 0, 0, 0.3);
                    margin-bottom: 2rem;
                }
                .form-grid {
                    display: grid;
                    grid-template-columns: 1fr 1fr;
                    gap: 1.5rem;
                }
                @media (max-width: 640px) {
                    .form-grid {
                        grid-template-columns: 1fr;
                    }
                }
                .form-group {
                    display: flex;
                    flex-direction: column;
                    gap: 0.5rem;
                }
                .form-group.full-width {
                    grid-column: span 2;
                }
                @media (max-width: 640px) {
                    .form-group.full-width {
                        grid-column: span 1;
                    }
                }
                label {
                    font-size: 0.875rem;
                    font-weight: 600;
                    color: #cbd5e1;
                    letter-spacing: 0.05em;
                    text-transform: uppercase;
                }
                input {
                    background: rgba(15, 23, 42, 0.6);
                    border: 1px solid rgba(148, 163, 184, 0.3);
                    border-radius: 8px;
                    padding: 0.75rem 1rem;
                    color: #f8fafc;
                    font-size: 1rem;
                    transition: all 0.2s ease;
                }
                input:focus {
                    outline: none;
                    border-color: #6366f1;
                    box-shadow: 0 0 0 3px rgba(99, 102, 241, 0.3);
                }
                button {
                    background: linear-gradient(135deg, #6366f1 0%, #4f46e5 100%);
                    color: white;
                    border: none;
                    border-radius: 8px;
                    padding: 1rem 2rem;
                    font-size: 1rem;
                    font-weight: 700;
                    cursor: pointer;
                    transition: all 0.2s ease;
                    box-shadow: 0 4px 14px 0 rgba(99, 102, 241, 0.4);
                    margin-top: 1.5rem;
                    display: inline-flex;
                    align-items: center;
                    justify-content: center;
                    gap: 0.5rem;
                    width: 100%;
                }
                button:hover {
                    transform: translateY(-1px);
                    box-shadow: 0 6px 20px 0 rgba(99, 102, 241, 0.5);
                }
                button:active {
                    transform: translateY(1px);
                }
                .error-alert {
                    background: rgba(239, 68, 68, 0.15);
                    border: 1px solid rgba(239, 68, 68, 0.4);
                    border-radius: 12px;
                    padding: 1rem 1.5rem;
                    color: #fca5a5;
                    margin-bottom: 2rem;
                    font-weight: 500;
                }
                .results-header {
                    font-size: 1.5rem;
                    font-weight: 700;
                    margin-bottom: 1.5rem;
                    border-bottom: 2px solid rgba(99, 102, 241, 0.2);
                    padding-bottom: 0.5rem;
                }
                .events-list {
                    display: flex;
                    flex-direction: column;
                    gap: 1rem;
                }
                .event-card {
                    background: rgba(15, 23, 42, 0.4);
                    border: 1px solid rgba(255, 255, 255, 0.05);
                    border-radius: 12px;
                    padding: 1.25rem;
                    transition: transform 0.2s ease, border-color 0.2s ease;
                }
                .event-card:hover {
                    transform: translateX(4px);
                    border-color: rgba(99, 102, 241, 0.4);
                }
                .event-title {
                    font-size: 1.2rem;
                    font-weight: 700;
                    margin: 0 0 0.5rem 0;
                    color: #f8fafc;
                }
                .event-time {
                    font-size: 0.875rem;
                    color: #a78bfa;
                    display: flex;
                    gap: 0.5rem;
                    align-items: center;
                    margin-bottom: 0.5rem;
                }
                .event-desc {
                    font-size: 0.95rem;
                    color: #cbd5e1;
                    margin: 0;
                    line-height: 1.5;
                }
                .no-events {
                    text-align: center;
                    color: #94a3b8;
                    padding: 3rem;
                    font-size: 1.1rem;
                }
                "#
            </style>
        </head>
        <body>
            <div class="container">
                <header>
                    <h1>"CalDAV Server Test Client"</h1>
                    <p class="subtitle">"Test CalDAV server connectivity, authentication, and retrieve event lists"</p>
                </header>

                <div class="card">
                    <form method="GET" action="/caldav-ui">
                        <div class="form-grid">
                            <div class="form-group full-width">
                                <label for="server_url">"Server URL"</label>
                                <input type="url" id="server_url" name="server_url" value=s_url placeholder="http://localhost:8080" required=true />
                            </div>
                            <div class="form-group">
                                <label for="username">"Username"</label>
                                <input type="text" id="username" name="username" value=u_name placeholder="e.g. notion" />
                            </div>
                            <div class="form-group">
                                <label for="password">"Password"</label>
                                <input type="password" id="password" name="password" value=p_word placeholder="••••••••" />
                            </div>
                            <div class="form-group full-width">
                                <label for="calendar_path">"Calendar Path / Path to db"</label>
                                <input type="text" id="calendar_path" name="calendar_path" value=c_path placeholder="e.g. /cal/your-database-uuid" required=true />
                            </div>
                        </div>
                        <button type="submit">
                            "Fetch Calendar Events"
                        </button>
                    </form>
                </div>

                {error.map(|err| view! {
                    <div class="error-alert">
                        <strong>"Error: "</strong> {err}
                    </div>
                })}

                {events.map(|evs| view! {
                    <div class="card">
                        <div class="results-header">"Fetched Events (" {evs.len()} ")"</div>
                        {if evs.is_empty() {
                            view! {
                                <div class="no-events">
                                    "No events found in this calendar, or could not find VCALENDAR components."
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="events-list">
                                    {evs.into_iter().map(|ev| view! {
                                        <div class="event-card">
                                            <div class="event-title">{ev.summary}</div>
                                            <div class="event-time">
                                                <span>"📅"</span>
                                                <span>{ev.start} " to " {ev.end}</span>
                                            </div>
                                            {if !ev.description.is_empty() {
                                                view! {
                                                    <p class="event-desc">{ev.description}</p>
                                                }.into_any()
                                            } else {
                                                "".into_any()
                                            }}
                                        </div>
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                })}
            </div>
        </body>
        </html>
    }
}

pub async fn ui_handler(
    Query(params): Query<UiParams>,
) -> impl IntoResponse {
    let mut error = None;
    let mut events = None;

    if let Some(ref server_url) = params.server_url {
        let username = params.username.clone().unwrap_or_default();
        let password = params.password.clone().unwrap_or_default();
        let calendar_path = params.calendar_path.clone().unwrap_or_default();

        match fetch_caldav_events(server_url, &username, &password, &calendar_path).await {
            Ok(evs) => {
                events = Some(evs);
            }
            Err(err) => {
                error = Some(err);
            }
        }
    }

    let server_url = params.server_url;
    let username = params.username;
    let password = params.password;
    let calendar_path = params.calendar_path;

    let html = (view! {
        <CalDavUi
            server_url=server_url
            username=username
            password=password
            calendar_path=calendar_path
            error=error
            events=events
        />
    }).to_html();

    Html(format!("<!DOCTYPE html>{}", html))
}
