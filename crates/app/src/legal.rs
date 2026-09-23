use leptos::prelude::*;
use leptos_router::components::A;

#[component]
fn LegalPage(
    title: &'static str,
    other_href: &'static str,
    other_label: &'static str,
    children: Children,
) -> impl IntoView {
    let page_title = format!("{title} — NotionCal");
    view! {
        <leptos_meta::Html attr:lang="en"/>
        <leptos_meta::Title text=page_title/>
        <div id="legal-root" class="bg-background text-on-surface min-h-screen">
            <main class="max-w-[720px] mx-auto px-margin-mobile md:px-margin-desktop pt-[64px] pb-lg space-y-lg">
                <div class="flex justify-end">
                    <A href=other_href attr:class="text-label-md text-secondary hover:underline">{other_label}</A>
                </div>
                {children()}
            </main>
            <div class="max-w-[1280px] mx-auto w-full px-margin-desktop">
                <crate::page_shell::PageFooter/>
            </div>
        </div>
    }
}

#[component]
pub fn PrivacyRoutePage() -> impl IntoView {
    view! {
        <LegalPage title="Privacy Policy" other_href="/terms" other_label="Terms of Service">
            <h1 class="text-h1 font-semibold text-primary mb-1">"Privacy Policy"</h1>
            <p class="text-label-md text-on-surface-variant mb-xl">"Last updated: 2026-08-02"</p>

            <p class="mb-lg">"NotionCal (\"the Service\") turns a Notion database into a CalDAV feed and a browser calendar view. This page explains what data we collect and how we use it."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"What we collect"</h2>
            <ul class="list-disc pl-lg space-y-xs mb-lg">
                <li><strong>"Account: "</strong>"the email address from your login (via our self-hosted Keycloak identity provider) — used only to identify your account."</li>
                <li><strong>"Notion access: "</strong>"when you connect your Notion workspace, we store the OAuth access token Notion issues us, along with your workspace id/name and integration bot id. This token is what lets the Service read and write the specific pages you granted access to."</li>
                <li><strong>"Calendar configuration: "</strong>"for each Notion database you choose to sync, we store its database id, the date property used for scheduling, and a display name."</li>
                <li><strong>"CalDAV credentials: "</strong>"we generate a random username/password per calendar so you can subscribe from Apple/Google Calendar or any CalDAV client. Only a salted hash (Argon2) of the password is stored — the plaintext is shown to you once, at creation time, and never again."</li>
            </ul>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"What we don't collect"</h2>
            <ul class="list-disc pl-lg space-y-xs mb-lg">
                <li>"No payment or billing information — the Service is free."</li>
                <li>"No analytics or advertising trackers."</li>
                <li>"No data is sold or shared with third parties for marketing."</li>
            </ul>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"How we use it"</h2>
            <p class="mb-lg">"Solely to operate the Service: fetching events from your Notion database, converting them to CalDAV/iCalendar format, keeping them in sync (via periodic polling and Notion's webhook events), and rendering the calendar webview so you can view and edit events yourself."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Where it's stored"</h2>
            <p class="mb-lg">"All data lives in a private PostgreSQL database we operate, not exposed to the public internet, reachable only by the Service itself. We do not use third-party data processors beyond Notion's own API (needed to read/write your workspace) and the infrastructure hosting our servers."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Your controls"</h2>
            <ul class="list-disc pl-lg space-y-xs mb-lg">
                <li>"You can revoke the Service's access at any time from Notion's own \"Connections\" settings in your workspace — this immediately invalidates the access token we hold."</li>
                <li>"To delete your account and all associated data (connections, calendars, credentials), email us at the address below."</li>
            </ul>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Contact"</h2>
            <p>"Questions about this policy: "<a class="text-secondary underline" href="mailto:huyvu8051@gmail.com">"huyvu8051@gmail.com"</a></p>
        </LegalPage>
    }
}

#[component]
pub fn TermsRoutePage() -> impl IntoView {
    view! {
        <LegalPage title="Terms of Service" other_href="/privacy" other_label="Privacy Policy">
            <h1 class="text-h1 font-semibold text-primary mb-1">"Terms of Service"</h1>
            <p class="text-label-md text-on-surface-variant mb-xl">"Last updated: 2026-08-02"</p>

            <p class="mb-lg">"By using NotionCal (\"the Service\"), you agree to these terms."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"The Service"</h2>
            <p class="mb-lg">"The Service connects to a Notion workspace you authorize, and exposes the database(s) you choose as a CalDAV feed and a browser-based calendar view. It is provided free of charge, with no guaranteed uptime or support response time."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Your responsibilities"</h2>
            <ul class="list-disc pl-lg space-y-xs mb-lg">
                <li>"You're responsible for the content of the Notion pages you connect, and for keeping your CalDAV credentials confidential."</li>
                <li>"Don't use the Service to store or distribute illegal content, or in a way that places excessive load on it (e.g. automated scraping outside normal calendar-client sync behavior)."</li>
                <li>"You must have the right to grant the Service access to any Notion workspace you connect."</li>
            </ul>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"No warranty"</h2>
            <p class="mb-lg">"The Service is provided \"as is,\" without warranty of any kind. We don't guarantee it will be uninterrupted, error-free, or that data will never be lost — Notion remains the source of truth for your data, and we recommend not relying on the Service as your only backup of important events."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Termination"</h2>
            <p class="mb-lg">"We may suspend or terminate access to the Service for any account found abusing it (as described above), or discontinue the Service entirely. You may stop using the Service and revoke its Notion access at any time."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Changes"</h2>
            <p class="mb-lg">"We may update these terms as the Service evolves; continued use after a change means you accept the updated terms."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Governing law"</h2>
            <p class="mb-lg">"These terms are governed by the laws of Vietnam."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Contact"</h2>
            <p><a class="text-secondary underline" href="mailto:huyvu8051@gmail.com">"huyvu8051@gmail.com"</a></p>
        </LegalPage>
    }
}
