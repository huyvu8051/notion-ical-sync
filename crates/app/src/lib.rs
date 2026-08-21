//! Phase-0 retry of the full Leptos SSR+CSR migration abandoned in
//! `~/.claude/plans/mighty-scribbling-floyd.md` (commit b2e7a26). That
//! attempt used a hand-wired `view! {}.to_html()` on the server +
//! `leptos::mount::hydrate_body` on the client, bypassing leptos_axum's real
//! SSR pipeline entirely — and hit a reproducible
//! `tachys::hydration::failed_to_cast_element` panic that survived three
//! structurally different fixes. The plan's own conclusion: prove
//! `leptos_axum`'s real SSR pipeline (`render_app_to_stream`) works
//! standalone, without `LeptosRoutes` owning the router, before migrating
//! any real page again.
//!
//! Phase 0 (`TestApp`, mounted at the throwaway `/dev/leptos-check`, not
//! linked from anywhere) proved the mechanism itself: SSR via
//! `leptos_axum::render_app_to_stream` + client hydration both work,
//! verified locally and in production (see git log for that verification).
//!
//! Phase 1 (`sync_log`) is the first real page: real DB data + real OIDC
//! auth, read-only. It proves the harder part the plan flagged — the
//! SSR→CSR *data* flow (design decision 3): the server serializes the
//! fetched rows once into the SSR'd HTML, and the client-side bootstrap
//! script reads that same blob back and re-mounts an identical component
//! tree with it, rather than re-fetching. If the client rendered with
//! different data than the server did, hydration would mismatch the DOM
//! tachys expects to find and panic — same failure class as the abandoned
//! attempt, just triggered by a data mismatch instead of a structural one.

pub mod sync_log;

use leptos::prelude::*;

/// Must be called exactly once, before the host binds any route that renders
/// this crate's components — `render_app_to_stream` schedules reactive work
/// onto a global executor that cargo-leptos apps normally get initialized
/// for free via `LeptosRoutes`. Since this crate is wired in with a plain
/// `.route(path, get(render_app_to_stream(Shell)))` instead (see Cargo.toml
/// feature comment), that init never happens unless the host does it.
#[cfg(feature = "ssr")]
pub fn init_executor() {
    any_spawner::Executor::init_tokio().expect("failed to init leptos reactive executor");
}

/// The whole HTML document, including `<html>`/`<head>`/`<body>` — this is
/// what gets passed to `leptos_axum::render_app_to_stream`. The wasm loader
/// script lives in `<head>` (not as a `<body>` sibling of `<TestApp/>`) so
/// that `<body>`'s children exactly match what `TestApp` renders — a bare
/// sibling script tag in `<body>` was one of the failure modes in the
/// abandoned attempt (see module docs).
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

/// Single reactive counter, single root element (no page-sized tuple of
/// top-level siblings — that shape was the other failure mode in the
/// abandoned attempt). If the count increments on click after WASM loads,
/// hydration genuinely attached; if clicking does nothing, or the console
/// shows a hydration panic, the mechanism is still broken.
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

// Named export, not `#[wasm_bindgen(start)]` — this crate now hosts more
// than one page's worth of components in a single wasm bundle (see
// sync_log), each needing to hydrate a different root component, so each
// page's own bootstrap script calls its own named function after `init()`
// instead of relying on a single fixed auto-run hydrate target.
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate_test_app() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(TestApp);
}
