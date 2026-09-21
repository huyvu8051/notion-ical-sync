#![recursion_limit = "256"]

pub mod confirm_button;
pub mod connect_notion;
pub mod landing;
pub mod legal;
pub mod me;
pub mod pick_databases;
pub mod sync_log;
pub mod webview;

use leptos::prelude::*;

#[cfg(feature = "ssr")]
pub fn init_executor() {
    any_spawner::Executor::init_tokio().expect("failed to init leptos reactive executor");
}

#[component]
pub fn Shell() -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <title>"Leptos SSR+CSR mechanism check"</title>
                <script type="module">
                    "import init, { hydrate_test_app } from '/pkg/app.js'; init('/pkg/app_bg.wasm').then(hydrate_test_app);"
                </script>
            </head>
            <body>
                <TestApp/>
            </body>
        </html>
    }
}

#[component]
pub fn TestApp() -> impl IntoView {
    let (count, set_count) = signal(0);

    view! {
        <div id="app-root" style="font-family: -apple-system, sans-serif; max-width: 480px; margin: 3rem auto; padding: 0 1.25rem;">
            <h1>"Leptos SSR+CSR mechanism check"</h1>
            <p>"Server-rendered via leptos_axum::render_app_to_stream. If the count below increments on click, hydration attached correctly."</p>
            <button
                style="font-size: 1rem; padding: 0.5rem 1rem;"
                on:click=move |_| set_count.update(|c| *c += 1)
            >
                "Clicked " {move || count.get()} " times"
            </button>
        </div>
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_test_app() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(TestApp);
}
