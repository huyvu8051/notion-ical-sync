#![recursion_limit = "256"]

pub mod confirm_button;
pub mod connect_notion;
pub mod landing;
pub mod legal;
pub mod me;
pub mod page_shell;
pub mod pick_databases;
pub mod sync_log;
pub mod webview;

use leptos::prelude::*;
use leptos_meta::{MetaTags, Title};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::StaticSegment;

#[cfg(feature = "ssr")]
pub fn init_executor() {
    any_spawner::Executor::init_tokio().expect("failed to init leptos reactive executor");
}

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
fn NotYetMigratedFallback() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        let location = leptos_router::hooks::use_location();
        Effect::new(move |_| {
            let pathname = location.pathname.get();
            let search = location.search.get();
            let path = if search.is_empty() {
                pathname
            } else {
                format!("{pathname}?{search}")
            };
            if let Some(window) = web_sys::window() {
                let _ = window.location().replace(&path);
            }
        });
    }
}

#[component]
pub fn App() -> impl IntoView {
    leptos_meta::provide_meta_context();
    view! {
        <Title text="NotionCal"/>
        <Router>
            <Routes fallback=NotYetMigratedFallback>
                <Route path=StaticSegment("") view=landing::LandingRoutePage/>
                <Route path=StaticSegment("privacy") view=legal::PrivacyRoutePage/>
                <Route path=StaticSegment("terms") view=legal::TermsRoutePage/>
                <Route path=(StaticSegment("connect"), StaticSegment("notion")) view=connect_notion::ConnectNotionRoutePage/>
                <Route path=(StaticSegment("connect"), StaticSegment("notion"), StaticSegment("databases")) view=pick_databases::PickDatabasesRoutePage/>
                <Route path=StaticSegment("me") view=me::MeRoutePage/>
                <Route path=(StaticSegment("app"), leptos_router::ParamSegment("public_id")) view=webview::WebviewRoutePage/>
                <Route path=(StaticSegment("me"), StaticSegment("calendars"), leptos_router::ParamSegment("public_id"), StaticSegment("log")) view=sync_log::SyncLogRoutePage/>
            </Routes>
        </Router>
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    webview::install_js_bridge();
    leptos::mount::hydrate_body(App);
}
