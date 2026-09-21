use leptos::ev::Event;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct CandidateData {
    pub icon: String,
    pub title: String,
    pub database_id: String,
    pub date_property: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PickDatabasesPageData {
    pub top_nav_html: String,
    pub connection_id: i64,
    pub candidates: Vec<CandidateData>,
}

#[component]
pub fn PickDatabasesShell(data: PickDatabasesPageData) -> impl IntoView {
    let json = serde_json::to_string(&data).unwrap_or_default();
    let script_breakout_safe_json = json.replace('<', "\\u003c");
    let inline_data_script =
        format!("window.__PICK_DATABASES_DATA__ = {script_breakout_safe_json};");

    let head_html = format!(
        r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Chọn cơ sở dữ liệu — NotionCal</title><link rel="stylesheet" href="/assets/style-auth-a.css"><link href="{fonts}" rel="stylesheet"><style>{style}</style><script>{data_script}</script><script type="module">import init, {{ hydrate_pick_databases }} from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(() => hydrate_pick_databases(JSON.stringify(window.__PICK_DATABASES_DATA__)));</script>"#,
        fonts = crate::connect_notion::GOOGLE_FONTS_HREF,
        style = crate::connect_notion::ONBOARDING_HEAD_STYLE,
        data_script = inline_data_script,
    );

    view! {
        <!DOCTYPE html>
        <html lang="vi">
            <head inner_html=head_html></head>
            <body class="min-h-screen flex flex-col">
                <PickDatabasesPage data=data/>
            </body>
        </html>
    }
}

#[component]
pub fn PickDatabasesPage(data: PickDatabasesPageData) -> impl IntoView {
    let top_nav_html = data.top_nav_html.clone();

    if data.candidates.is_empty() {
        return view! {
            <div id="pick-databases-root">
                <div inner_html=top_nav_html></div>
                <main class="flex-grow flex flex-col items-center justify-center px-margin-mobile text-center">
                    <h1 class="text-h1 text-primary mb-sm">"Chọn cơ sở dữ liệu để đồng bộ"</h1>
                    <p class="text-on-surface-variant text-body-lg">
                        "Không tìm thấy cơ sở dữ liệu nào bạn đã cấp quyền. "
                        <a class="text-secondary underline" href="/connect/notion/start">"Cấp thêm quyền truy cập trên Notion"</a>
                        "."
                    </p>
                </main>
            </div>
        }
        .into_any();
    }

    let syncable_checked_signals: Vec<Option<RwSignal<bool>>> = data
        .candidates
        .iter()
        .map(|c| c.date_property.is_some().then(|| RwSignal::new(true)))
        .collect();

    let checked_count = {
        let syncable_checked_signals = syncable_checked_signals.clone();
        Memo::new(move |_| {
            syncable_checked_signals
                .iter()
                .filter(|s| s.map(|s| s.get()).unwrap_or(false))
                .count()
        })
    };

    let connection_id = data.connection_id.to_string();

    let rows = data
        .candidates
        .into_iter()
        .zip(syncable_checked_signals)
        .map(|(c, sig)| match (c.date_property, sig) {
            (Some(date_prop), Some(sig)) => view! {
                <label class="group flex items-center gap-md p-md bg-white border border-outline-variant rounded-lg cursor-pointer hover:border-primary transition-all duration-200 card-shadow">
                    <input
                        type="checkbox"
                        name="db_ids"
                        value=c.database_id
                        prop:checked=move || sig.get()
                        on:change=move |ev: Event| sig.set(event_target_checked(&ev))
                        class="w-5 h-5 border-2 border-outline-variant rounded-sm text-primary focus:ring-0 focus:ring-offset-0 custom-checkbox"
                    />
                    <div class="flex items-center justify-center w-10 h-10 bg-surface-container rounded-lg text-xl">{c.icon}</div>
                    <div class="flex-grow">
                        <h3 class="text-h3 text-primary">{c.title}</h3>
                        <p class="text-on-surface-variant text-label-md flex items-center gap-xs">
                            <span class="material-symbols-outlined text-[14px]">"calendar_today"</span>
                            {format!("Có thuộc tính ngày: {date_prop}")}
                        </p>
                    </div>
                </label>
            }
            .into_any(),
            (None, _) => view! {
                <div class="flex items-center gap-md p-md bg-surface-container-low border border-outline-variant opacity-60 rounded-lg grayscale cursor-not-allowed">
                    <input type="checkbox" disabled=true class="w-5 h-5 border-2 border-outline-variant rounded-sm bg-surface-container-highest cursor-not-allowed"/>
                    <div class="flex items-center justify-center w-10 h-10 bg-surface-container-high rounded-lg text-xl">{c.icon}</div>
                    <div class="flex-grow">
                        <div class="flex items-center gap-sm">
                            <h3 class="text-h3 text-on-surface-variant">{c.title}</h3>
                            <span class="bg-error-container text-on-error-container text-[10px] px-xs py-[2px] rounded font-bold uppercase tracking-wider">"Lỗi"</span>
                        </div>
                        <p class="text-error text-label-md flex items-center gap-xs mt-1">
                            <span class="material-symbols-outlined text-[14px]">"warning"</span>
                            "Không tìm thấy thuộc tính ngày"
                        </p>
                    </div>
                </div>
            }
            .into_any(),
            (Some(_), None) => unreachable!("a syncable candidate always gets a signal"),
        })
        .collect_view();

    view! {
        <div id="pick-databases-root">
            <div inner_html=top_nav_html></div>
            <main class="flex-grow flex flex-col pt-lg pb-32">
                <div class="max-w-[720px] mx-auto w-full px-margin-mobile md:px-0">
                    <section class="mb-xl">
                        <h1 class="text-h1 text-primary mb-sm">"Chọn cơ sở dữ liệu để đồng bộ"</h1>
                        <p class="text-on-surface-variant text-body-lg">"Chúng tôi đã tìm thấy các cơ sở dữ liệu sau trong không gian làm việc Notion của bạn. Chọn (các) cơ sở dữ liệu bạn muốn biến thành lịch."</p>
                    </section>
                    <form method="post" action="/connect/notion/databases">
                        <input type="hidden" name="connection_id" value=connection_id/>
                        <div class="space-y-md">{rows}</div>
                        <div class="mt-xl text-center">
                            <a class="text-on-surface-variant hover:text-primary transition-colors text-label-md" href="/connect/notion/start">"Không thấy cơ sở dữ liệu bạn cần? Cấp thêm quyền truy cập trên Notion"</a>
                        </div>
                        <div class="fixed bottom-0 left-0 right-0 bg-white/80 backdrop-blur-md border-t border-outline-variant py-md z-40">
                            <div class="max-w-[1280px] mx-auto px-margin-desktop flex justify-between items-center">
                                <a class="px-lg h-[40px] border border-outline-variant text-primary text-label-md rounded hover:bg-surface-container-low transition-colors flex items-center gap-sm" href="/me">
                                    <span class="material-symbols-outlined text-[18px]">"arrow_back"</span>
                                    "Quay lại"
                                </a>
                                <button
                                    type="submit"
                                    id="continue-btn"
                                    disabled=move || checked_count.get() == 0
                                    class="px-lg h-[40px] bg-primary text-white text-label-md rounded hover:opacity-90 transition-all flex items-center gap-sm active:scale-95 shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                    {move || if checked_count.get() > 0 {
                                        format!("Tiếp tục với {} cơ sở dữ liệu", checked_count.get())
                                    } else {
                                        "Chọn ít nhất 1 cơ sở dữ liệu".to_string()
                                    }}
                                    <span class="material-symbols-outlined text-[18px]">
                                        {move || if checked_count.get() > 0 { "arrow_forward" } else { "error" }}
                                    </span>
                                </button>
                            </div>
                        </div>
                    </form>
                </div>
            </main>
        </div>
    }
    .into_any()
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_pick_databases(json: String) {
    console_error_panic_hook::set_once();
    let data: PickDatabasesPageData =
        serde_json::from_str(&json).expect("invalid pick-databases page payload from server");
    leptos::mount::hydrate_body(move || view! { <PickDatabasesPage data=data.clone()/> });
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_data() -> PickDatabasesPageData {
        PickDatabasesPageData {
            top_nav_html: "<header>nav</header>".to_string(),
            connection_id: 42,
            candidates: vec![
                CandidateData {
                    icon: "📄".to_string(),
                    title: "Work <Tasks>".to_string(),
                    database_id: "db-1".to_string(),
                    date_property: Some("Due date".to_string()),
                },
                CandidateData {
                    icon: "🗒".to_string(),
                    title: "No date property".to_string(),
                    database_id: "db-2".to_string(),
                    date_property: None,
                },
            ],
        }
    }

    #[test]
    fn renders_candidates_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <PickDatabasesPage data=sample_data()/> }.to_html());
        assert!(html.contains("db-1"));
        assert!(html.contains("Due date"));
        assert!(html.contains("Không tìm thấy thuộc tính ngày"));
        assert!(html.contains("&lt;Tasks&gt;") || html.contains("Work"));
    }

    #[test]
    fn renders_empty_state_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let data = PickDatabasesPageData {
            top_nav_html: "<header>nav</header>".to_string(),
            connection_id: 1,
            candidates: vec![],
        };
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <PickDatabasesPage data=data/> }.to_html());
        assert!(html.contains("Không tìm thấy cơ sở dữ liệu"));
    }

    #[test]
    fn shell_is_script_breakout_safe() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = leptos::prelude::Owner::new()
            .with(|| view! { <PickDatabasesShell data=sample_data()/> }.to_html());
        assert!(!html.contains("</script><script>alert"));
    }
}
