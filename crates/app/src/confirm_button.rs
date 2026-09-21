//! Native `crates/app` port of `crates/islands`' two confirm-button widgets
//! (Phase B2/B3 island-folding work) — ordinary child components with local
//! `signal()`-driven "armed" state, hydrated as part of whatever page tree
//! they're mounted in, rather than the separate `leptos-island` custom
//! element + `hydrate_islands()` mechanism `crates/islands` used. Behavior
//! is otherwise identical (click once to arm, 3s window to confirm, same
//! "replaces a blocking native confirm() dialog" reasoning) — ported
//! near-verbatim from `crates/islands/src/lib.rs`.

use leptos::html;
use leptos::prelude::*;
use std::time::Duration;

/// Submits a real `<form>` on confirm — for actions whose target is known at
/// render time (delete/regenerate on `/me`), so the server-side handler this
/// posts to needs no changes at all.
#[component]
pub fn ConfirmButton(
    action: String,
    label: String,
    confirm_label: String,
    class: String,
) -> impl IntoView {
    let (confirming, set_confirming) = signal(false);
    let form_ref = NodeRef::<html::Form>::new();

    let on_click = move |_| {
        if confirming.get() {
            if let Some(form) = form_ref.get() {
                let _ = form.submit();
            }
        } else {
            set_confirming.set(true);
            set_timeout(move || set_confirming.set(false), Duration::from_secs(3));
        }
    };

    view! {
        <form node_ref=form_ref action=action method="post">
            <button type="button" class=class on:click=on_click>
                {move || if confirming.get() { confirm_label.clone() } else { label.clone() }}
            </button>
        </form>
    }
}

#[cfg(feature = "hydrate")]
fn call_global_fn(name: &str) {
    use wasm_bindgen::JsCast;
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(val) = js_sys::Reflect::get(&win, &wasm_bindgen::JsValue::from_str(name)) else {
        return;
    };
    if let Ok(func) = val.dyn_into::<js_sys::Function>() {
        let _ = func.call0(&win);
    }
}
#[cfg(not(feature = "hydrate"))]
fn call_global_fn(_name: &str) {}

/// Like `ConfirmButton`, but the confirmed action calls a global JS function
/// by name instead of submitting a form — for webview.rs's delete-event
/// modal, where the actual target (the currently-open event's id) is only
/// known client-side at click time, not at render time.
#[component]
pub fn ConfirmActionButton(
    id: String,
    label: String,
    confirm_label: String,
    class: String,
    on_confirm_fn: String,
) -> impl IntoView {
    let (confirming, set_confirming) = signal(false);

    let on_click = move |_| {
        if confirming.get() {
            set_confirming.set(false);
            call_global_fn(&on_confirm_fn);
        } else {
            set_confirming.set(true);
            set_timeout(move || set_confirming.set(false), Duration::from_secs(3));
        }
    };

    view! {
        <button type="button" id=id class=class on:click=on_click>
            {move || if confirming.get() { confirm_label.clone() } else { label.clone() }}
        </button>
    }
}
