//! The page for a URL that is not one of ours.
//!
//! Registered at `/404` so the server can render it by path, but nobody is
//! meant to arrive by typing that: the axum fallback renders this page while
//! leaving the address the visitor actually asked for in the bar, and sends
//! it with a 404 status so crawlers do not index a typo as a real page.

use leptos::prelude::*;
use leptos_router::components::A;

use crate::page_shell::PageFooter;

#[component]
pub fn NotFoundRoutePage() -> impl IntoView {
    view! {
        <leptos_meta::Html attr:lang="en"/>
        <leptos_meta::Title text="Page not found — NotionCal"/>
        // Nothing here is worth indexing, and the address that produced it is
        // whatever the visitor mistyped.
        <leptos_meta::Meta name="robots" content="noindex"/>
        <div id="not-found-root" class="bg-background text-on-surface min-h-screen flex flex-col">
            // pt-[64px] like the legal pages, not an arbitrary value of its
            // own -- the committed stylesheet is stale and only carries the
            // classes already in use, so a new one renders as nothing here
            // and correctly in production, which is a difference nobody
            // should have to discover twice.
            <main class="flex-1 max-w-[720px] w-full mx-auto px-margin-mobile md:px-margin-desktop pt-[64px] pb-lg">
                <p class="text-label-md text-on-surface-variant mb-sm">"404"</p>
                <h1 class="text-h1 font-semibold text-primary mb-md">"This page does not exist"</h1>
                <p class="mb-xl text-on-surface-variant">
                    "The address may have a typo in it, or it may be a link to something we have since moved. Nothing is wrong with your account."
                </p>
                <div class="flex flex-wrap gap-sm">
                    <A href="/" attr:class="bg-primary text-on-primary px-md h-10 text-label-md flex items-center justify-center hover:opacity-90 transition-all active:scale-95">
                        "Back to the home page"
                    </A>
                    // Someone signed in who mistypes is far more likely to
                    // want their calendars than the sales pitch.
                    <A href="/me" attr:class="bg-surface border border-outline-variant text-primary px-md h-10 text-label-md flex items-center justify-center hover:border-outline transition-all active:scale-95">
                        "Your calendars"
                    </A>
                </div>
            </main>
            <div class="max-w-[1280px] mx-auto w-full px-margin-desktop">
                <PageFooter/>
            </div>
        </div>
    }
}
