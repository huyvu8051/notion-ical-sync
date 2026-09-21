//! Native `crates/app` port of `crates/islands::ConfirmButton` (Phase B2
//! island-folding work) — an ordinary child component with local
//! `signal()`-driven "armed" state, hydrated as part of whatever page tree
//! it's mounted in, rather than the separate `leptos-island` custom element
//! + `hydrate_islands()` mechanism `crates/islands` used. Behavior is
//! otherwise identical (click once to arm, 3s window to confirm, same
//! "replaces a blocking native confirm() dialog" reasoning) — ported
//! near-verbatim from `crates/islands/src/lib.rs`.
//!
//! `crates/islands` also had a `ConfirmActionButton` variant (confirmed
//! action calls a global JS function instead of submitting a form) for
//! webview.rs's delete-event modal. It is deliberately *not* ported here:
//! folding it in caused a real, browser-verified
//! `tachys::hydration::failed_to_cast_element` panic, isolated by
//! experiment to the component itself rather than surrounding DOM
//! structure. Since that button doesn't need real Rust-side reactivity,
//! webview.rs reimplements the same UX as plain JS instead — see its
//! module doc.

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
