#![cfg_attr(not(feature = "hydrate"), allow(dead_code, unused_variables))]

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "hydrate")]
use std::cell::RefCell;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebviewJsConfig {
    pub events_url: String,
    pub alert_update_date_failed: String,
    pub locale: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct WebviewLabels {
    pub modal_title_add: String,
    pub modal_title_edit: String,
    pub event_name_label: String,
    pub event_name_placeholder: String,
    pub start_label: String,
    pub end_label: String,
    pub allday_label: String,
    pub location_label: String,
    pub location_placeholder: String,
    pub location_no_results: String,
    pub notes_label: String,
    pub notes_placeholder: String,
    pub priority_label: String,
    pub priority_none: String,
    pub priority_high: String,
    pub priority_medium: String,
    pub priority_low: String,
    pub busy_label: String,
    pub busy_unset: String,
    pub busy_busy: String,
    pub busy_free: String,
    pub reminder_label: String,
    pub reminder_none: String,
    pub reminder_5min: String,
    pub reminder_15min: String,
    pub reminder_30min: String,
    pub reminder_1hour: String,
    pub reminder_1day: String,
    pub reminder_2days: String,
    pub reminder_1week: String,
    pub reminder_custom: String,
    pub reminder_custom_minutes: String,
    pub reminder_custom_hours: String,
    pub reminder_custom_days: String,
    pub travel_time_label: String,
    pub travel_none: String,
    pub travel_0min: String,
    pub travel_15min: String,
    pub travel_30min: String,
    pub travel_45min: String,
    pub travel_1hour: String,
    pub travel_90min: String,
    pub travel_custom: String,
    pub open_in_notion: String,
    pub delete_btn: String,
    pub cancel_btn: String,
    pub save_btn: String,
    pub saving_label: String,
    pub confirm_delete_event: String,
    pub repeat_display_prefix: String,
    pub attendees_display_prefix: String,
    pub alert_enter_title: String,
    pub alert_pick_start: String,
    pub alert_update_failed: String,
    pub alert_create_failed: String,
    pub alert_delete_failed: String,
    pub alert_quota_exceeded: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WebviewPageData {
    pub html_lang: String,
    pub title: String,
    pub header_html: String,
    pub events_url: String,
    pub mapbox_token: Option<String>,
    pub labels: WebviewLabels,
    pub js_config: WebviewJsConfig,
}

const WEBVIEW_HEAD_STYLE: &str = r#"
body { background-color: #fbf9f9; color: #1b1c1c; -webkit-font-smoothing: antialiased; }
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; font-size: 20px; }
.modal-shadow { box-shadow: 0px 4px 12px rgba(0, 0, 0, 0.05); }
#calendar { max-width: 1100px; margin: 0 auto; padding: 24px; }
.fc { --fc-border-color: #c4c7c7; --fc-button-bg-color: #fff; --fc-button-border-color: #c4c7c7; --fc-button-text-color: #1b1c1c;
  --fc-button-hover-bg-color: #f0f0f0; --fc-button-hover-border-color: #9a9a9a;
  --fc-button-active-bg-color: #000; --fc-button-active-border-color: #000; --fc-today-bg-color: #f5f3f3; font-family: 'Inter', sans-serif; }
.fc .fc-button { box-shadow: none !important; text-transform: none; font-weight: 500; }
.fc .fc-button-primary:not(:disabled).fc-button-active,
.fc .fc-button-primary:not(:disabled):active { color: #fff; }
@media (max-width: 640px) {
  #calendar { padding: 12px; }
  .fc-header-toolbar { flex-wrap: wrap; row-gap: 8px; justify-content: center !important; }
  .fc-toolbar-chunk { display: flex; flex-wrap: wrap; justify-content: center; gap: 4px; }
  .fc-toolbar-title { font-size: 1.1em !important; }
  .fc .fc-button { padding: 4px 8px !important; font-size: 0.8em !important; }
  header.h-16 { padding-left: 12px; padding-right: 12px; }
  header.h-16 .text-h1 { font-size: 18px; max-width: 40vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
}
@media (max-width: 420px) {
  header.h-16 .back-label { display: none; }
}
"#;

use crate::page_shell::GOOGLE_FONTS_HREF;

#[component]
pub fn WebviewShell(data: WebviewPageData) -> impl IntoView {
    let html_lang = data.html_lang.clone();

    let json = serde_json::to_string(&data).unwrap_or_default();
    let json_safe = json.replace('<', "\\u003c");
    let inline_data_script = format!("window.__WEBVIEW_DATA__ = {json_safe};");

    let fc_locale_script = if data.js_config.locale == "vi" {
        r#"<script src="https://cdn.jsdelivr.net/npm/@fullcalendar/core@6.1.15/locales/vi.global.min.js"></script>"#
    } else {
        ""
    };

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title} — NotionCal</title><link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.css"><script src="https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js"></script>{fc_locale_script}<link rel="stylesheet" href="/assets/style-auth-a.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{data_script}</script><script src="/static/webview.js" defer></script><script type="module">import init, {{ hydrate_webview, webview_open_create_modal, webview_open_edit_modal }} from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => {{ hydrate_webview(JSON.stringify(window.__WEBVIEW_DATA__)); window.webview_open_create_modal = webview_open_create_modal; window.webview_open_edit_modal = webview_open_edit_modal; }});</script>"#,
        title = data.title,
        fonts = GOOGLE_FONTS_HREF,
        style = WEBVIEW_HEAD_STYLE,
        data_script = inline_data_script,
    );

    view! {
        <!DOCTYPE html>
        <html lang=html_lang>
            <head inner_html=head_html></head>
            <body class="bg-background text-on-surface">
                <WebviewPage data=data/>
            </body>
        </html>
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditEventData {
    id: String,
    title: String,
    start: String,
    end: Option<String>,
    all_day: bool,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    busy: Option<bool>,
    #[serde(default)]
    reminder_minutes: Option<i64>,
    #[serde(default)]
    travel_minutes: Option<i64>,
    #[serde(default)]
    repeat_rule: Option<String>,
    #[serde(default)]
    attendees: Vec<String>,
    #[serde(default)]
    notion_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum ModalRequest {
    None,
    Create {
        start: String,
        end: String,
        all_day: bool,
    },
    Edit(Box<EditEventData>),
}

#[cfg(feature = "hydrate")]
thread_local! {
    static MODAL_REQUEST_SETTER: RefCell<Option<WriteSignal<ModalRequest>>> = RefCell::new(None);
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn webview_open_create_modal(start: String, end: String, all_day: bool) {
    MODAL_REQUEST_SETTER.with(|cell| {
        if let Some(setter) = cell.borrow().as_ref() {
            setter.set(ModalRequest::Create { start, end, all_day });
        }
    });
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn webview_open_edit_modal(event_json: String) {
    let Ok(data) = serde_json::from_str::<EditEventData>(&event_json) else {
        return;
    };
    MODAL_REQUEST_SETTER.with(|cell| {
        if let Some(setter) = cell.borrow().as_ref() {
            setter.set(ModalRequest::Edit(Box::new(data)));
        }
    });
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = window, js_name = webview_refetch_calendar_events)]
    fn refetch_calendar_events();
}

fn to_date_only(s: &str) -> String {
    s.chars().take(10).collect()
}

fn to_datetime_local(s: &str) -> String {
    if s.len() >= 16 {
        s.chars().take(16).collect()
    } else if s.len() == 10 {
        format!("{s}T00:00")
    } else {
        s.to_string()
    }
}

fn reminder_to_preset(minutes: Option<i64>) -> (&'static str, String, &'static str) {
    match minutes {
        None => ("", String::new(), "minutes"),
        Some(5) => ("5", String::new(), "minutes"),
        Some(15) => ("15", String::new(), "minutes"),
        Some(30) => ("30", String::new(), "minutes"),
        Some(60) => ("60", String::new(), "minutes"),
        Some(1440) => ("1440", String::new(), "minutes"),
        Some(2880) => ("2880", String::new(), "minutes"),
        Some(10080) => ("10080", String::new(), "minutes"),
        Some(m) if m % 1440 == 0 => ("custom", (m / 1440).to_string(), "days"),
        Some(m) if m % 60 == 0 => ("custom", (m / 60).to_string(), "hours"),
        Some(m) => ("custom", m.to_string(), "minutes"),
    }
}

fn reminder_preset_to_minutes(preset: &str, custom_value: &str, custom_unit: &str) -> Option<i64> {
    match preset {
        "" => None,
        "custom" => {
            let n: i64 = custom_value.trim().parse().ok()?;
            Some(match custom_unit {
                "days" => n * 1440,
                "hours" => n * 60,
                _ => n,
            })
        }
        preset => preset.parse().ok(),
    }
}

fn travel_to_preset(minutes: Option<i64>) -> (&'static str, String, &'static str) {
    match minutes {
        None => ("", String::new(), "minutes"),
        Some(0) => ("0", String::new(), "minutes"),
        Some(15) => ("15", String::new(), "minutes"),
        Some(30) => ("30", String::new(), "minutes"),
        Some(45) => ("45", String::new(), "minutes"),
        Some(60) => ("60", String::new(), "minutes"),
        Some(90) => ("90", String::new(), "minutes"),
        Some(m) if m % 1440 == 0 => ("custom", (m / 1440).to_string(), "days"),
        Some(m) if m % 60 == 0 => ("custom", (m / 60).to_string(), "hours"),
        Some(m) => ("custom", m.to_string(), "minutes"),
    }
}

fn travel_preset_to_minutes(preset: &str, custom_value: &str, custom_unit: &str) -> Option<i64> {
    match preset {
        "" => None,
        "custom" => {
            let n: i64 = custom_value.trim().parse().ok()?;
            Some(match custom_unit {
                "days" => n * 1440,
                "hours" => n * 60,
                _ => n,
            })
        }
        preset => preset.parse().ok(),
    }
}

#[derive(Clone, Deserialize)]
struct MapboxResponse {
    #[serde(default)]
    features: Vec<MapboxFeature>,
}

#[derive(Clone, Deserialize)]
struct MapboxFeature {
    properties: MapboxProperties,
}

#[derive(Clone, Deserialize)]
struct MapboxProperties {
    #[serde(default)]
    full_address: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    place_formatted: Option<String>,
}

impl MapboxFeature {
    fn display_text(&self) -> String {
        if let Some(addr) = &self.properties.full_address {
            return addr.clone();
        }
        match (&self.properties.name, &self.properties.place_formatted) {
            (Some(n), Some(p)) => format!("{n}, {p}"),
            (Some(n), None) => n.clone(),
            (None, Some(p)) => p.clone(),
            (None, None) => String::new(),
        }
    }
}

#[derive(Serialize)]
struct EventPayload {
    title: String,
    start: String,
    end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    busy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reminder_minutes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    travel_minutes: Option<i64>,
}

#[component]
pub fn WebviewPage(data: WebviewPageData) -> impl IntoView {
    let header_html = data.header_html.clone();
    let mapbox_token = data.mapbox_token.clone();

    let visible = RwSignal::new(false);
    let editing_id = RwSignal::new(None::<String>);
    let notion_url = RwSignal::new(None::<String>);
    let repeat_display = RwSignal::new(None::<String>);
    let attendees_display = RwSignal::new(None::<String>);

    let title = RwSignal::new(String::new());
    let start = RwSignal::new(String::new());
    let end = RwSignal::new(String::new());
    let all_day = RwSignal::new(false);
    let location = RwSignal::new(String::new());
    let location_suggestions = RwSignal::new(Vec::<MapboxFeature>::new());
    let location_show_suggestions = RwSignal::new(false);
    let location_gen = RwSignal::new(0u32);
    let notes = RwSignal::new(String::new());
    let priority = RwSignal::new(String::new());
    let busy = RwSignal::new(String::new());
    let reminder_preset = RwSignal::new(String::new());
    let reminder_custom_value = RwSignal::new(String::new());
    let reminder_custom_unit = RwSignal::new("minutes".to_string());
    let travel_preset = RwSignal::new(String::new());
    let travel_custom_value = RwSignal::new(String::new());
    let travel_custom_unit = RwSignal::new("minutes".to_string());
    let delete_armed = RwSignal::new(false);
    let saving = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let (modal_request, set_modal_request) = signal(ModalRequest::None);
    #[cfg(feature = "hydrate")]
    MODAL_REQUEST_SETTER.with(|cell| *cell.borrow_mut() = Some(set_modal_request));
    #[cfg(not(feature = "hydrate"))]
    let _ = set_modal_request;

    Effect::new(move |_| {
        match modal_request.get() {
            ModalRequest::None => {}
            ModalRequest::Create {
                start: s,
                end: e,
                all_day: ad,
            } => {
                editing_id.set(None);
                title.set(String::new());
                start.set(if s.is_empty() {
                    String::new()
                } else if ad {
                    to_date_only(&s)
                } else {
                    to_datetime_local(&s)
                });
                end.set(if e.is_empty() {
                    String::new()
                } else if ad {
                    to_date_only(&e)
                } else {
                    to_datetime_local(&e)
                });
                all_day.set(ad);
                location.set(String::new());
                notes.set(String::new());
                priority.set(String::new());
                busy.set(String::new());
                reminder_preset.set(String::new());
                reminder_custom_value.set(String::new());
                reminder_custom_unit.set("minutes".to_string());
                travel_preset.set(String::new());
                travel_custom_value.set(String::new());
                travel_custom_unit.set("minutes".to_string());
                notion_url.set(None);
                repeat_display.set(None);
                attendees_display.set(None);
                delete_armed.set(false);
                error.set(None);
                visible.set(true);
            }
            ModalRequest::Edit(data) => {
                editing_id.set(Some(data.id.clone()));
                title.set(data.title.clone());
                start.set(if data.all_day {
                    to_date_only(&data.start)
                } else {
                    to_datetime_local(&data.start)
                });
                end.set(match &data.end {
                    Some(e) if data.all_day => to_date_only(e),
                    Some(e) => to_datetime_local(e),
                    None => String::new(),
                });
                all_day.set(data.all_day);
                location.set(data.location.clone().unwrap_or_default());
                notes.set(data.notes.clone().unwrap_or_default());
                priority.set(data.priority.map(|p| p.to_string()).unwrap_or_default());
                busy.set(match data.busy {
                    Some(true) => "true".to_string(),
                    Some(false) => "false".to_string(),
                    None => String::new(),
                });
                let (rp, rc, ru) = reminder_to_preset(data.reminder_minutes);
                reminder_preset.set(rp.to_string());
                reminder_custom_value.set(rc);
                reminder_custom_unit.set(ru.to_string());
                let (tp, tc, tu) = travel_to_preset(data.travel_minutes);
                travel_preset.set(tp.to_string());
                travel_custom_value.set(tc);
                travel_custom_unit.set(tu.to_string());
                notion_url.set(data.notion_url.clone());
                repeat_display.set(data.repeat_rule.clone());
                attendees_display.set((!data.attendees.is_empty()).then(|| data.attendees.join(", ")));
                delete_armed.set(false);
                error.set(None);
                visible.set(true);
            }
        }
    });

    let close_modal = move |_| visible.set(false);

    let on_all_day_toggle = move |ev: leptos::ev::Event| {
        let checked = event_target_checked(&ev);
        all_day.set(checked);
        if checked {
            start.update(|s| *s = to_date_only(s));
            end.update(|s| {
                if !s.is_empty() {
                    *s = to_date_only(s);
                }
            });
        } else {
            start.update(|s| *s = to_datetime_local(s));
            end.update(|s| {
                if !s.is_empty() {
                    *s = to_datetime_local(s);
                }
            });
        }
    };

    let on_location_input = move |ev: leptos::ev::Event| {
        let value = event_target_value(&ev);
        location.set(value.clone());
        location_gen.update(|g| *g = g.wrapping_add(1));
        if value.trim().len() < 3 {
            location_suggestions.set(Vec::new());
            location_show_suggestions.set(false);
            return;
        }
        #[cfg(feature = "hydrate")]
        {
            let my_gen = location_gen.get_untracked();
            let Some(token) = mapbox_token.clone() else {
                return;
            };
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(300).await;
                if location_gen.get_untracked() != my_gen {
                    return;
                }
                let query = js_sys::encode_uri_component(&value).as_string().unwrap_or_default();
                let url = format!(
                    "https://api.mapbox.com/search/geocode/v6/forward?q={query}&autocomplete=true&limit=5&access_token={token}"
                );
                let resp = gloo_net::http::Request::get(&url).send().await;
                if location_gen.get_untracked() != my_gen {
                    return;
                }
                match resp {
                    Ok(r) if r.ok() => {
                        if let Ok(parsed) = r.json::<MapboxResponse>().await {
                            location_show_suggestions.set(!parsed.features.is_empty());
                            location_suggestions.set(parsed.features);
                        }
                    }
                    _ => {
                        location_suggestions.set(Vec::new());
                        location_show_suggestions.set(false);
                    }
                }
            });
        }
    };

    let pick_suggestion = move |text: String| {
        location.set(text);
        location_show_suggestions.set(false);
        location_suggestions.set(Vec::new());
    };
    let location_no_results = StoredValue::new(data.labels.location_no_results.clone());

    let events_url_stored = StoredValue::new(data.events_url.clone());
    let alert_delete_failed_stored = StoredValue::new(data.labels.alert_delete_failed.clone());
    let do_delete = move || {
        saving.set(true);
        #[cfg(feature = "hydrate")]
        {
            let Some(id) = editing_id.get_untracked() else {
                return;
            };
            let url = format!("{}/{}", events_url_stored.get_value(), urlencoding_minimal(&id));
            let alert_delete_failed = alert_delete_failed_stored.get_value();
            wasm_bindgen_futures::spawn_local(async move {
                let result = gloo_net::http::Request::delete(&url).send().await;
                saving.set(false);
                match result {
                    Ok(r) if r.ok() => {
                        visible.set(false);
                        refetch_calendar_events();
                    }
                    _ => error.set(Some(alert_delete_failed)),
                }
            });
        }
    };

    let on_delete_click = move |_| {
        if delete_armed.get_untracked() {
            delete_armed.set(false);
            do_delete();
        } else {
            delete_armed.set(true);
            #[cfg(feature = "hydrate")]
            {
                let armed_at = delete_armed;
                wasm_bindgen_futures::spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(3000).await;
                    armed_at.set(false);
                });
            }
        }
    };

    let events_url_for_save = data.events_url.clone();
    let l_for_save = data.labels.clone();
    let on_save = move |_| {
        let title_v = title.get_untracked().trim().to_string();
        if title_v.is_empty() {
            error.set(Some(l_for_save.alert_enter_title.clone()));
            return;
        }
        let start_raw = start.get_untracked();
        if start_raw.is_empty() {
            error.set(Some(l_for_save.alert_pick_start.clone()));
            return;
        }
        let ad = all_day.get_untracked();
        let start_v = if ad {
            start_raw.clone()
        } else {
            local_to_utc_iso(&start_raw)
        };
        let end_raw = end.get_untracked();
        let end_v = if end_raw.is_empty() {
            None
        } else if ad {
            Some(end_raw.clone())
        } else {
            Some(local_to_utc_iso(&end_raw))
        };

        let location_v = {
            let v = location.get_untracked();
            (!v.trim().is_empty()).then_some(v)
        };
        let notes_v = {
            let v = notes.get_untracked();
            (!v.trim().is_empty()).then_some(v)
        };
        let priority_v = priority.get_untracked().parse::<u8>().ok();
        let busy_v = match busy.get_untracked().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        let reminder_v = reminder_preset_to_minutes(
            &reminder_preset.get_untracked(),
            &reminder_custom_value.get_untracked(),
            &reminder_custom_unit.get_untracked(),
        );
        let travel_v = travel_preset_to_minutes(
            &travel_preset.get_untracked(),
            &travel_custom_value.get_untracked(),
            &travel_custom_unit.get_untracked(),
        );

        saving.set(true);
        error.set(None);
        #[cfg(feature = "hydrate")]
        {
        let payload = EventPayload {
            title: title_v,
            start: start_v,
            end: end_v,
            location: location_v,
            notes: notes_v,
            priority: priority_v,
            busy: busy_v,
            reminder_minutes: reminder_v,
            travel_minutes: travel_v,
        };
        let editing = editing_id.get_untracked();
        let base_url = events_url_for_save.clone();
        let alert_update_failed = l_for_save.alert_update_failed.clone();
        let alert_create_failed = l_for_save.alert_create_failed.clone();
        let alert_quota_exceeded = l_for_save.alert_quota_exceeded.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match &editing {
                Some(id) => {
                    let url = format!("{base_url}/{}", urlencoding_minimal(id));
                    gloo_net::http::Request::patch(&url)
                        .json(&payload)
                        .expect("payload always serializes")
                        .send()
                        .await
                }
                None => gloo_net::http::Request::post(&base_url)
                    .json(&payload)
                    .expect("payload always serializes")
                    .send()
                    .await,
            };
            saving.set(false);
            match result {
                Ok(r) if r.ok() => {
                    visible.set(false);
                    refetch_calendar_events();
                }
                Ok(r) if r.status() == 429 => error.set(Some(alert_quota_exceeded)),
                Ok(_) => {
                    error.set(Some(if editing.is_some() {
                        alert_update_failed
                    } else {
                        alert_create_failed
                    }))
                }
                Err(_) => {
                    error.set(Some(if editing.is_some() {
                        alert_update_failed
                    } else {
                        alert_create_failed
                    }))
                }
            }
        });
        }
    };

    let l1 = data.labels.clone();
    let l2 = data.labels.clone();
    let l3 = data.labels.clone();
    let l4 = data.labels.clone();
    let l5 = data.labels.clone();
    let unit_minutes_label = StoredValue::new(data.labels.reminder_custom_minutes.clone());
    let unit_hours_label = StoredValue::new(data.labels.reminder_custom_hours.clone());
    let unit_days_label = StoredValue::new(data.labels.reminder_custom_days.clone());

    view! {
        <div id="webview-root">
            <div inner_html=header_html></div>
            <div id="calendar"></div>

            <div
                class="fixed inset-0 bg-black/40 z-50 items-center justify-center"
                class:hidden=move || !visible.get()
                class:flex=move || visible.get()
            >
                <div class="bg-white w-full max-w-lg mx-4 rounded-xl modal-shadow overflow-hidden max-h-[90vh] overflow-y-auto">
                    <div class="px-lg py-md border-b border-outline-variant flex items-center justify-between">
                        <h2 class="text-h2">
                            {move || if editing_id.get().is_some() { l1.modal_title_edit.clone() } else { l1.modal_title_add.clone() }}
                        </h2>
                        <button class="p-1 hover:bg-surface-container-low rounded-lg transition-colors" on:click=close_modal>
                            <span class="material-symbols-outlined">close</span>
                        </button>
                    </div>
                    <div class="p-lg space-y-lg">
                        {move || error.get().map(|e| view! {
                            <div class="p-sm bg-error-container text-on-error-container text-label-md rounded-lg">{e}</div>
                        })}
                        <div class="space-y-xs">
                            <label class="text-label-md text-on-surface-variant">{l2.event_name_label.clone()}</label>
                            <input
                                class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                placeholder=l2.event_name_placeholder.clone()
                                type="text"
                                prop:value=move || title.get()
                                on:input=move |ev| title.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="grid grid-cols-2 gap-md">
                            <div class="space-y-xs">
                                <label class="text-label-md text-on-surface-variant">{l2.start_label.clone()}</label>
                                <input
                                    class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                    type=move || if all_day.get() { "date" } else { "datetime-local" }
                                    prop:value=move || start.get()
                                    on:input=move |ev| start.set(event_target_value(&ev))
                                />
                            </div>
                            <div class="space-y-xs">
                                <label class="text-label-md text-on-surface-variant">{l2.end_label.clone()}</label>
                                <input
                                    class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                    type=move || if all_day.get() { "date" } else { "datetime-local" }
                                    prop:value=move || end.get()
                                    on:input=move |ev| end.set(event_target_value(&ev))
                                />
                            </div>
                        </div>
                        <div class="flex items-center gap-sm">
                            <input
                                class="w-4 h-4 rounded text-primary focus:ring-primary border-outline-variant"
                                type="checkbox"
                                prop:checked=move || all_day.get()
                                on:change=on_all_day_toggle
                            />
                            <label class="cursor-pointer">{l2.allday_label.clone()}</label>
                        </div>
                        <div class="space-y-xs relative">
                            <label class="text-label-md text-on-surface-variant">{l3.location_label.clone()}</label>
                            <input
                                class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                placeholder=l3.location_placeholder.clone()
                                type="text"
                                prop:value=move || location.get()
                                on:input=on_location_input
                            />
                            <Show when=move || location_show_suggestions.get()>
                                <div class="absolute z-10 mt-1 w-full bg-white border border-outline-variant rounded-lg shadow-lg max-h-48 overflow-y-auto">
                                    {move || {
                                        let suggestions = location_suggestions.get();
                                        if suggestions.is_empty() {
                                            view! { <div class="px-md py-sm text-label-md text-on-surface-variant">{location_no_results.get_value()}</div> }.into_any()
                                        } else {
                                            suggestions.into_iter().map(|f| {
                                                let text = f.display_text();
                                                let text_for_click = text.clone();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="w-full text-left px-md py-sm text-body-md hover:bg-surface-container-low transition-colors"
                                                        on:click=move |_| pick_suggestion(text_for_click.clone())
                                                    >
                                                        {text}
                                                    </button>
                                                }
                                            }).collect_view().into_any()
                                        }
                                    }}
                                </div>
                            </Show>
                        </div>
                        <div class="space-y-xs">
                            <label class="text-label-md text-on-surface-variant">{l3.notes_label.clone()}</label>
                            <textarea
                                class="w-full px-md py-sm border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                placeholder=l3.notes_placeholder.clone()
                                rows="2"
                                prop:value=move || notes.get()
                                on:input=move |ev| notes.set(event_target_value(&ev))
                            ></textarea>
                        </div>
                        <div class="grid grid-cols-2 gap-md">
                            <div class="space-y-xs">
                                <label class="text-label-md text-on-surface-variant">{l4.priority_label.clone()}</label>
                                <select
                                    class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                    prop:value=move || priority.get()
                                    on:change=move |ev| priority.set(event_target_value(&ev))
                                >
                                    <option value="">{l4.priority_none.clone()}</option>
                                    <option value="1">{l4.priority_high.clone()}</option>
                                    <option value="5">{l4.priority_medium.clone()}</option>
                                    <option value="9">{l4.priority_low.clone()}</option>
                                </select>
                            </div>
                            <div class="space-y-xs">
                                <label class="text-label-md text-on-surface-variant">{l4.busy_label.clone()}</label>
                                <select
                                    class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                    prop:value=move || busy.get()
                                    on:change=move |ev| busy.set(event_target_value(&ev))
                                >
                                    <option value="">{l4.busy_unset.clone()}</option>
                                    <option value="true">{l4.busy_busy.clone()}</option>
                                    <option value="false">{l4.busy_free.clone()}</option>
                                </select>
                            </div>
                        </div>
                        <div class="space-y-xs">
                            <label class="text-label-md text-on-surface-variant">{l4.reminder_label.clone()}</label>
                            <select
                                class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                prop:value=move || reminder_preset.get()
                                on:change=move |ev| reminder_preset.set(event_target_value(&ev))
                            >
                                <option value="">{l4.reminder_none.clone()}</option>
                                <option value="5">{l4.reminder_5min.clone()}</option>
                                <option value="15">{l4.reminder_15min.clone()}</option>
                                <option value="30">{l4.reminder_30min.clone()}</option>
                                <option value="60">{l4.reminder_1hour.clone()}</option>
                                <option value="1440">{l4.reminder_1day.clone()}</option>
                                <option value="2880">{l4.reminder_2days.clone()}</option>
                                <option value="10080">{l4.reminder_1week.clone()}</option>
                                <option value="custom">{l4.reminder_custom.clone()}</option>
                            </select>
                            <Show when=move || reminder_preset.get() == "custom">
                                <div class="flex gap-sm mt-xs">
                                    <input
                                        class="w-24 h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                        type="number"
                                        min="0"
                                        prop:value=move || reminder_custom_value.get()
                                        on:input=move |ev| reminder_custom_value.set(event_target_value(&ev))
                                    />
                                    <select
                                        class="flex-1 h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                        prop:value=move || reminder_custom_unit.get()
                                        on:change=move |ev| reminder_custom_unit.set(event_target_value(&ev))
                                    >
                                        <option value="minutes">{unit_minutes_label.get_value()}</option>
                                        <option value="hours">{unit_hours_label.get_value()}</option>
                                        <option value="days">{unit_days_label.get_value()}</option>
                                    </select>
                                </div>
                            </Show>
                        </div>
                        <div class="space-y-xs">
                            <label class="text-label-md text-on-surface-variant">{l5.travel_time_label.clone()}</label>
                            <select
                                class="w-full h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                prop:value=move || travel_preset.get()
                                on:change=move |ev| travel_preset.set(event_target_value(&ev))
                            >
                                <option value="">{l5.travel_none.clone()}</option>
                                <option value="0">{l5.travel_0min.clone()}</option>
                                <option value="15">{l5.travel_15min.clone()}</option>
                                <option value="30">{l5.travel_30min.clone()}</option>
                                <option value="45">{l5.travel_45min.clone()}</option>
                                <option value="60">{l5.travel_1hour.clone()}</option>
                                <option value="90">{l5.travel_90min.clone()}</option>
                                <option value="custom">{l5.travel_custom.clone()}</option>
                            </select>
                            <Show when=move || travel_preset.get() == "custom">
                                <div class="flex gap-sm mt-xs">
                                    <input
                                        class="w-24 h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                        type="number"
                                        min="0"
                                        prop:value=move || travel_custom_value.get()
                                        on:input=move |ev| travel_custom_value.set(event_target_value(&ev))
                                    />
                                    <select
                                        class="flex-1 h-10 px-md border border-outline-variant rounded-lg focus:border-secondary focus:ring-1 focus:ring-secondary/10 outline-none transition-all"
                                        prop:value=move || travel_custom_unit.get()
                                        on:change=move |ev| travel_custom_unit.set(event_target_value(&ev))
                                    >
                                        <option value="minutes">{unit_minutes_label.get_value()}</option>
                                        <option value="hours">{unit_hours_label.get_value()}</option>
                                        <option value="days">{unit_days_label.get_value()}</option>
                                    </select>
                                </div>
                            </Show>
                        </div>
                        {move || repeat_display.get().map(|r| view! {
                            <div class="text-label-md text-on-surface-variant">{format!("{}{}", data.labels.repeat_display_prefix, r)}</div>
                        })}
                        {move || attendees_display.get().map(|a| view! {
                            <div class="text-label-md text-on-surface-variant">{format!("{}{}", data.labels.attendees_display_prefix, a)}</div>
                        })}
                        {move || notion_url.get().map(|url| view! {
                            <a class="flex items-center gap-xs text-secondary hover:underline text-label-md" href=url target="_blank" rel="noopener">
                                {data.labels.open_in_notion.clone()}
                                <span class="material-symbols-outlined text-[14px]">open_in_new</span>
                            </a>
                        })}
                    </div>
                    <div class="px-lg py-md bg-surface-container-low flex items-center justify-between">
                        <Show when=move || editing_id.get().is_some()>
                            <button type="button" class="text-error text-label-md hover:underline" on:click=on_delete_click>
                                {
                                    let delete_btn = data.labels.delete_btn.clone();
                                    let confirm_delete_event = data.labels.confirm_delete_event.clone();
                                    move || if delete_armed.get() { confirm_delete_event.clone() } else { delete_btn.clone() }
                                }
                            </button>
                        </Show>
                        <div class="flex items-center gap-md ml-auto">
                            <button class="px-md h-10 border border-outline-variant rounded-lg bg-white hover:bg-surface-container-low text-label-md transition-colors" on:click=close_modal>
                                {data.labels.cancel_btn.clone()}
                            </button>
                            <button
                                class="bg-primary text-on-primary px-lg h-10 rounded-lg text-label-md hover:opacity-90 transition-opacity disabled:opacity-50"
                                prop:disabled=move || saving.get()
                                on:click=on_save
                            >
                                {move || if saving.get() { data.labels.saving_label.clone() } else { data.labels.save_btn.clone() }}
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}

fn urlencoding_minimal(s: &str) -> String {
    #[cfg(feature = "hydrate")]
    {
        js_sys::encode_uri_component(s).as_string().unwrap_or_default()
    }
    #[cfg(not(feature = "hydrate"))]
    {
        s.to_string()
    }
}

fn local_to_utc_iso(local_value: &str) -> String {
    #[cfg(feature = "hydrate")]
    {
        let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(local_value));
        if date.get_time().is_nan() {
            return local_value.to_string();
        }
        date.to_iso_string().as_string().unwrap_or_else(|| local_value.to_string())
    }
    #[cfg(not(feature = "hydrate"))]
    {
        local_value.to_string()
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_webview(json: String) {
    console_error_panic_hook::set_once();
    let data: WebviewPageData =
        serde_json::from_str(&json).expect("invalid webview page payload from server");
    leptos::mount::hydrate_body(move || view! { <WebviewPage data=data.clone()/> });
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_labels() -> WebviewLabels {
        WebviewLabels {
            modal_title_add: "Thêm sự kiện".to_string(),
            modal_title_edit: "Chỉnh sửa sự kiện".to_string(),
            event_name_label: "Tên sự kiện".to_string(),
            event_name_placeholder: "Nhập tên sự kiện...".to_string(),
            start_label: "Bắt đầu".to_string(),
            end_label: "Kết thúc".to_string(),
            allday_label: "Cả ngày".to_string(),
            location_label: "Địa điểm".to_string(),
            location_placeholder: "Nhập địa điểm...".to_string(),
            location_no_results: "Không tìm thấy kết quả".to_string(),
            notes_label: "Ghi chú".to_string(),
            notes_placeholder: "Nhập ghi chú...".to_string(),
            priority_label: "Mức độ ưu tiên".to_string(),
            priority_none: "Không đặt".to_string(),
            priority_high: "Cao".to_string(),
            priority_medium: "Trung bình".to_string(),
            priority_low: "Thấp".to_string(),
            busy_label: "Trạng thái".to_string(),
            busy_unset: "Không đặt".to_string(),
            busy_busy: "Bận".to_string(),
            busy_free: "Rảnh".to_string(),
            reminder_label: "Nhắc nhở".to_string(),
            reminder_none: "Không nhắc".to_string(),
            reminder_5min: "5 phút trước".to_string(),
            reminder_15min: "15 phút trước".to_string(),
            reminder_30min: "30 phút trước".to_string(),
            reminder_1hour: "1 giờ trước".to_string(),
            reminder_1day: "1 ngày trước".to_string(),
            reminder_2days: "2 ngày trước".to_string(),
            reminder_1week: "1 tuần trước".to_string(),
            reminder_custom: "Tuỳ chỉnh...".to_string(),
            reminder_custom_minutes: "Phút".to_string(),
            reminder_custom_hours: "Giờ".to_string(),
            reminder_custom_days: "Ngày".to_string(),
            travel_time_label: "Thời gian di chuyển".to_string(),
            travel_none: "Không đặt".to_string(),
            travel_0min: "0 phút".to_string(),
            travel_15min: "15 phút".to_string(),
            travel_30min: "30 phút".to_string(),
            travel_45min: "45 phút".to_string(),
            travel_1hour: "1 giờ".to_string(),
            travel_90min: "1.5 giờ".to_string(),
            travel_custom: "Tuỳ chỉnh...".to_string(),
            open_in_notion: "Mở trong Notion".to_string(),
            delete_btn: "Xoá".to_string(),
            cancel_btn: "Huỷ".to_string(),
            save_btn: "Lưu".to_string(),
            saving_label: "Đang lưu...".to_string(),
            confirm_delete_event: "Xoá sự kiện này?".to_string(),
            repeat_display_prefix: "Lặp lại: ".to_string(),
            attendees_display_prefix: "Người được mời: ".to_string(),
            alert_enter_title: "Nhập tên sự kiện".to_string(),
            alert_pick_start: "Chọn ngày bắt đầu".to_string(),
            alert_update_failed: "Cập nhật thất bại".to_string(),
            alert_create_failed: "Tạo event thất bại".to_string(),
            alert_delete_failed: "Xoá thất bại".to_string(),
            alert_quota_exceeded: "Đã đạt giới hạn".to_string(),
        }
    }

    fn sample_data() -> WebviewPageData {
        WebviewPageData {
            html_lang: "vi".to_string(),
            title: "Work &lt;Calendar&gt;".to_string(),
            header_html: "<header>top</header>".to_string(),
            events_url: "/app/test-id/api/events".to_string(),
            mapbox_token: None,
            labels: sample_labels(),
            js_config: WebviewJsConfig {
                events_url: "/app/test-id/api/events".to_string(),
                alert_update_date_failed: "Cập nhật ngày thất bại".to_string(),
                locale: "vi".to_string(),
            },
        }
    }

    #[test]
    fn renders_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <WebviewPage data=sample_data()/> }.to_html());
        assert!(html.contains("id=\"calendar\""));
        assert!(html.contains("Thêm sự kiện"));
    }

    #[test]
    fn shell_embeds_data_and_is_script_breakout_safe() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <WebviewShell data=sample_data()/> }.to_html());
        assert!(html.contains("/app/test-id/api/events"));
        assert!(html.contains("/static/webview.js"));
        assert!(html.contains("fullcalendar@6.1.15"));
        assert!(html.contains("&lt;Calendar&gt;"));
        assert!(!html.contains("</script><script>alert"));
    }

    #[test]
    fn reminder_preset_roundtrip() {
        assert_eq!(reminder_preset_to_minutes("1440", "", "minutes"), Some(1440));
        assert_eq!(reminder_preset_to_minutes("custom", "3", "days"), Some(4320));
        assert_eq!(reminder_preset_to_minutes("custom", "2", "hours"), Some(120));
        assert_eq!(reminder_preset_to_minutes("", "", "minutes"), None);
        assert_eq!(reminder_to_preset(Some(1440)).0, "1440");
        assert_eq!(reminder_to_preset(Some(4320)), ("custom", "3".to_string(), "days"));
    }

    #[test]
    fn travel_preset_roundtrip() {
        assert_eq!(travel_preset_to_minutes("60", "", "minutes"), Some(60));
        assert_eq!(travel_preset_to_minutes("custom", "120", "minutes"), Some(120));
        assert_eq!(travel_preset_to_minutes("custom", "2", "hours"), Some(120));
        assert_eq!(travel_preset_to_minutes("custom", "3", "days"), Some(4320));
        assert_eq!(travel_preset_to_minutes("", "", "minutes"), None);
        assert_eq!(travel_to_preset(Some(45)).0, "45");
        assert_eq!(
            travel_to_preset(Some(100)),
            ("custom", "100".to_string(), "minutes")
        );
        assert_eq!(
            travel_to_preset(Some(4320)),
            ("custom", "3".to_string(), "days")
        );
    }
}
