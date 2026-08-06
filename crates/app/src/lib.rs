//! Whole-page Leptos components, server-rendered on first load (`ssr`
//! feature) and hydrated into a reactive client-side app after WASM loads
//! (`hydrate` feature) — see the plan at
//! `~/.claude/plans/mighty-scribbling-floyd.md`.
//!
//! Unlike `crates/islands` (partial hydration of small widgets inside
//! otherwise static HTML), a page here owns its *entire* `<body>` — the
//! surrounding `<html><head>...</head>` shell (title, meta tags, stylesheet
//! link) stays hand-written in the axum handler, but everything inside
//! `<body>` is real Leptos, so client-side hydration can attach to the whole
//! body in one piece instead of needing island-style partial mounting.

use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use wasm_bindgen::prelude::*;

/// Must be the last element of every page's view — the client-side hydration
/// bootstrap. It has to be part of the Leptos-rendered tree itself, not a
/// sibling tacked on outside it: `hydrate_body` expects `<body>`'s children
/// to exactly match what the component rendered, so a `<script>` tag added
/// by the surrounding hand-written HTML shell (after `{body}`) throws off
/// the hydration walker's element-by-element matching and crashes with
/// `tachys::hydration::failed_to_cast_element` the moment it reaches this
/// position in the tree.
#[component]
fn HydrationBootstrap() -> impl IntoView {
    view! { <script type="module">"import init from '/pkg/app.js'; init();"</script> }
}

/// A page-local "was this helpful" toggle — deliberately trivial (no network
/// call, no external JS), just enough real click-handling + reactive
/// re-render to prove hydration actually attached to a page that otherwise
/// has zero interactive elements today.
///
/// One persistent `<button>` whose *text* toggles (both closure branches
/// return a plain `String`), mirroring `crates/islands::ConfirmButton`
/// exactly — not `.into_any()`-erased branches switching between different
/// element shapes (a `<span>` vs. a `<button>`-containing fragment), which
/// panicked in `tachys::hydration::failed_to_cast_element` when hydrating a
/// whole page via `hydrate_body` (unlike islands' custom-element mount,
/// which freshly re-renders into its own element rather than matching
/// pre-existing DOM node-by-node, `hydrate_body` hydration is strict about
/// matching SSR'd markup exactly, and doesn't tolerate erased-type branches
/// here).
#[component]
fn HelpfulToggle() -> impl IntoView {
    let (answered, set_answered) = signal(false);

    view! {
        <p class="helpful">
            <button type="button" on:click=move |_| set_answered.set(true)>
                {move || {
                    if answered.get() {
                        "Thanks for the feedback!".to_string()
                    } else {
                        "Was this page clear? 👍 Yes".to_string()
                    }
                }}
            </button>
        </p>
    }
}

#[component]
pub fn PrivacyPage() -> impl IntoView {
    view! {
        <div class="top-nav">
            <strong>"NotionCal"</strong>
            <a class="back" href="/me">
                "← Back"
            </a>
        </div>
        <h1>"Privacy Policy"</h1>
        <p class="updated">"Last updated: 2026-08-02"</p>

        <p>
            "NotionCal (\"the Service\") turns a Notion database into a CalDAV feed and
            a browser calendar view. This page explains what data we collect and how we use it."
        </p>

        <h2>"What we collect"</h2>
        <ul>
            <li>
                <strong>"Account: "</strong>
                "the email address from your login (via our
                self-hosted Keycloak identity provider) — used only to identify your account."
            </li>
            <li>
                <strong>"Notion access: "</strong>
                "when you connect your Notion workspace, we
                store the OAuth access token Notion issues us, along with your workspace id/name
                and integration bot id. This token is what lets the Service read and write the
                specific pages you granted access to."
            </li>
            <li>
                <strong>"Calendar configuration: "</strong>
                "for each Notion database you choose
                to sync, we store its database id, the date property used for scheduling, and a
                display name."
            </li>
            <li>
                <strong>"CalDAV credentials: "</strong>
                "we generate a random username/password
                per calendar so you can subscribe from Apple/Google Calendar or any CalDAV
                client. Only a salted hash (Argon2) of the password is stored — the plaintext is
                shown to you once, at creation time, and never again."
            </li>
        </ul>

        <h2>"What we don't collect"</h2>
        <ul>
            <li>"No payment or billing information — the Service is free."</li>
            <li>"No analytics or advertising trackers."</li>
            <li>"No data is sold or shared with third parties for marketing."</li>
        </ul>

        <h2>"How we use it"</h2>
        <p>
            "Solely to operate the Service: fetching events from your Notion database,
            converting them to CalDAV/iCalendar format, keeping them in sync (via periodic
            polling and Notion's webhook events), and rendering the calendar webview so you
            can view and edit events yourself."
        </p>

        <h2>"Where it's stored"</h2>
        <p>
            "All data lives in a private PostgreSQL database we operate, not exposed to the
            public internet, reachable only by the Service itself. We do not use third-party
            data processors beyond Notion's own API (needed to read/write your workspace) and
            the infrastructure hosting our servers."
        </p>

        <h2>"Your controls"</h2>
        <ul>
            <li>
                "You can revoke the Service's access at any time from Notion's own
                \"Connections\" settings in your workspace — this immediately invalidates the
                access token we hold."
            </li>
            <li>
                "To delete your account and all associated data (connections, calendars,
                credentials), email us at the address below."
            </li>
        </ul>

        <h2>"Contact"</h2>
        <p>
            "Questions about this policy: "
            <a href="mailto:huyvu8051@gmail.com">"huyvu8051@gmail.com"</a>
        </p>

        <HelpfulToggle />
        <HydrationBootstrap />
    }
}

#[component]
pub fn TermsPage() -> impl IntoView {
    view! {
        <div class="top-nav">
            <strong>"NotionCal"</strong>
            <a class="back" href="/me">
                "← Back"
            </a>
        </div>
        <h1>"Terms of Service"</h1>
        <p class="updated">"Last updated: 2026-08-02"</p>

        <p>"By using NotionCal (\"the Service\"), you agree to these terms."</p>

        <h2>"The Service"</h2>
        <p>
            "The Service connects to a Notion workspace you authorize, and exposes the
            database(s) you choose as a CalDAV feed and a browser-based calendar view. It is
            provided free of charge, with no guaranteed uptime or support response time."
        </p>

        <h2>"Your responsibilities"</h2>
        <ul>
            <li>
                "You're responsible for the content of the Notion pages you connect, and for
                keeping your CalDAV credentials confidential."
            </li>
            <li>
                "Don't use the Service to store or distribute illegal content, or in a way
                that places excessive load on it (e.g. automated scraping outside normal
                calendar-client sync behavior)."
            </li>
            <li>
                "You must have the right to grant the Service access to any Notion workspace
                you connect."
            </li>
        </ul>

        <h2>"No warranty"</h2>
        <p>
            "The Service is provided \"as is,\" without warranty of any kind. We don't
            guarantee it will be uninterrupted, error-free, or that data will never be lost —
            Notion remains the source of truth for your data, and we recommend not relying on
            the Service as your only backup of important events."
        </p>

        <h2>"Termination"</h2>
        <p>
            "We may suspend or terminate access to the Service for any account found abusing
            it (as described above), or discontinue the Service entirely. You may stop using
            the Service and revoke its Notion access at any time."
        </p>

        <h2>"Changes"</h2>
        <p>
            "We may update these terms as the Service evolves; continued use after a change
            means you accept the updated terms."
        </p>

        <h2>"Governing law"</h2>
        <p>"These terms are governed by the laws of Vietnam."</p>

        <h2>"Contact"</h2>
        <p>
            <a href="mailto:huyvu8051@gmail.com">"huyvu8051@gmail.com"</a>
        </p>

        <HelpfulToggle />
        <HydrationBootstrap />
    }
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen(start)]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    let Some(page) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .and_then(|b| b.get_attribute("data-page"))
    else {
        return;
    };
    match page.as_str() {
        "privacy" => leptos::mount::hydrate_body(PrivacyPage),
        "terms" => leptos::mount::hydrate_body(TermsPage),
        _ => {}
    }
}
