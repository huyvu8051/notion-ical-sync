#![recursion_limit = "256"]

pub mod column_mapping;
pub mod confirm_button;
pub mod connect_notion;
pub mod edit_mapping;
pub mod guide;
pub mod landing;
pub mod legal;
pub mod me;
pub mod not_found;
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

/// What the client shows for a path this router has no route for.
///
/// It used to hard-reload, from when pages were being moved into Leptos one
/// at a time and a client-side navigation could land on one the server knew
/// and this router did not. Every page is in the table above now, and the
/// only paths left outside it are action endpoints -- /billing/*, /logout,
/// /connect/notion/start -- which are reached by a plain anchor and a full
/// page load anyway.
///
/// Keeping the reload once the server started rendering 404s was an infinite
/// loop: the server answers /pricing with this page, the client hydrates,
/// sees /pricing still matches nothing, reloads /pricing, and round it goes.
/// 233 requests in twelve seconds, invisible from the outside because every
/// pass drew the same page.
///
/// So the client now renders what the server rendered. The two agree, and
/// nothing reloads.
#[component]
fn NotYetMigratedFallback() -> impl IntoView {
    view! { <not_found::NotFoundRoutePage/> }
}

#[component]
fn HomeStyleHeader() -> impl IntoView {
    let location = leptos_router::hooks::use_location();
    let is_home_style_page = move || {
        matches!(
            location.pathname.get().as_str(),
            "/" | "/privacy" | "/terms" | "/refund" | "/guide"
        )
    };
    view! {
        <Show when=is_home_style_page fallback=|| ()>
            <page_shell::HomeHeader/>
        </Show>
    }
}

#[component]
pub fn App() -> impl IntoView {
    leptos_meta::provide_meta_context();
    view! {
        <Title text="NotionCal"/>
        <leptos_meta::Link rel="stylesheet" href="/assets/style-auth-a.css"/>
        <leptos_meta::Link href=page_shell::GOOGLE_FONTS_HREF rel="stylesheet"/>
        <leptos_meta::Style>{page_shell::ROOT_ICON_STYLE}</leptos_meta::Style>
        <leptos_meta::Style>{page_shell::HOME_HEADER_STYLE}</leptos_meta::Style>
        <Router>
            <HomeStyleHeader/>
            <Routes fallback=NotYetMigratedFallback>
                <Route path=StaticSegment("") view=landing::LandingRoutePage/>
                <Route path=StaticSegment("privacy") view=legal::PrivacyRoutePage/>
                <Route path=StaticSegment("terms") view=legal::TermsRoutePage/>
                <Route path=StaticSegment("refund") view=legal::RefundRoutePage/>
                <Route path=StaticSegment("guide") view=guide::GuideRoutePage/>
                // Rendered by the axum fallback for any unknown path, which
                // keeps the address the visitor typed. Reachable directly too;
                // that costs nothing and makes the page easy to look at.
                <Route path=StaticSegment("404") view=not_found::NotFoundRoutePage/>
                <Route path=(StaticSegment("connect"), StaticSegment("notion")) view=connect_notion::ConnectNotionRoutePage/>
                <Route path=(StaticSegment("connect"), StaticSegment("notion"), StaticSegment("databases")) view=pick_databases::PickDatabasesRoutePage/>
                <Route path=StaticSegment("me") view=me::MeRoutePage/>
                <Route path=(StaticSegment("app"), leptos_router::ParamSegment("public_id")) view=webview::WebviewRoutePage/>
                <Route path=(StaticSegment("me"), StaticSegment("calendars"), leptos_router::ParamSegment("public_id"), StaticSegment("log")) view=sync_log::SyncLogRoutePage/>
                <Route path=(StaticSegment("me"), StaticSegment("calendars"), leptos_router::ParamSegment("public_id"), StaticSegment("mapping")) view=edit_mapping::EditMappingRoutePage/>
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
