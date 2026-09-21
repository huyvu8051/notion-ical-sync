use leptos::html;
use leptos::prelude::*;
use std::time::Duration;

const CONFIRM_ARM_WINDOW: Duration = Duration::from_secs(3);

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
            set_timeout(move || set_confirming.set(false), CONFIRM_ARM_WINDOW);
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
