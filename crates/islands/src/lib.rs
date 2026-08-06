//! Small interactive widgets hydrated as Leptos islands — everything else in
//! the app stays hand-rolled `format!` HTML served straight from axum (see
//! plan: `~/.claude/plans/mighty-scribbling-floyd.md`). Only code reachable
//! from `#[island]` components here ships as WASM to the client.

use leptos::html;
use leptos::prelude::*;
use std::time::Duration;
#[cfg(feature = "hydrate")]
use wasm_bindgen::prelude::*;

/// Replaces a native `confirm()` dialog — which blocks all further JS
/// execution (and any CDP-driven browser automation) until a human dismisses
/// it — with a "click again within 3s to confirm" affordance on the button
/// itself. Renders a real `<form>` so the server-side handler this posts to
/// needs no changes at all.
#[island]
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
/// by name instead of submitting a form — for cases like webview.rs's
/// delete-event modal, where the actual target (the currently-open event's
/// id) is only known client-side at click time, not at server-render time.
#[island]
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

#[cfg(feature = "hydrate")]
#[wasm_bindgen(start)]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_islands();
}
