use leptos::prelude::*;
use leptos_router::components::A;

/// Every legal page, so each one can link to the others without any of them
/// holding a list of its siblings. Adding a fourth is one line here.
const LEGAL_PAGES: [(&str, &str); 3] = [
    ("/privacy", "Privacy Policy"),
    ("/terms", "Terms of Service"),
    ("/refund", "Refund Policy"),
];

#[component]
fn LegalPage(
    title: &'static str,
    description: &'static str,
    path: &'static str,
    children: Children,
) -> impl IntoView {
    let page_title = format!("{title} — NotionCal");
    let canonical_url = format!("https://notion-caldav.opendiy.vn{path}");
    view! {
        <leptos_meta::Html attr:lang="en"/>
        <leptos_meta::Title text=page_title.clone()/>
        <leptos_meta::Meta name="description" content=description/>
        <leptos_meta::Link rel="canonical" href=canonical_url.clone()/>
        <leptos_meta::Meta property="og:site_name" content="NotionCal"/>
        <leptos_meta::Meta property="og:type" content="website"/>
        <leptos_meta::Meta property="og:title" content=page_title.clone()/>
        <leptos_meta::Meta property="og:description" content=description/>
        <leptos_meta::Meta property="og:url" content=canonical_url/>
        <div id="legal-root" class="bg-background text-on-surface min-h-screen">
            <main class="max-w-[720px] mx-auto px-margin-mobile md:px-margin-desktop pt-[64px] pb-lg space-y-lg">
                <div class="flex justify-end gap-6">
                    {LEGAL_PAGES
                        .iter()
                        .filter(|(href, _)| *href != path)
                        .map(|(href, label)| {
                            view! {
                                <A href=*href attr:class="text-label-md text-secondary hover:underline">{*label}</A>
                            }
                        })
                        .collect_view()}
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
        <LegalPage
            title="Privacy Policy"
            description="How NotionCal collects, stores, and uses your data when you sync a Notion database to a CalDAV calendar."
            path="/privacy"
        >
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

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Payment data"</h2>
            <p class="mb-lg">"Paddle.com is our payment provider and the merchant of record for every purchase. Your card details are entered on Paddle's checkout and are never sent to us or stored by us — we never see them. What we keep is what Paddle tells us afterwards: the customer and subscription identifiers it assigns you, your subscription status, and the date your free period started. That is enough to know whether your account is paid, and nothing more."</p>
            <p class="mb-lg">"Paddle processes this data as its own controller under its "<a class="text-secondary underline" href="https://www.paddle.com/legal/privacy">"privacy policy"</a>"."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"What we don't collect"</h2>
            <ul class="list-disc pl-lg space-y-xs mb-lg">
                <li>"No card numbers, bank details, or billing addresses — those stay with Paddle."</li>
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
        <LegalPage
            title="Terms of Service"
            description="The terms that apply when you use NotionCal to sync a Notion database with a CalDAV calendar app."
            path="/terms"
        >
            <h1 class="text-h1 font-semibold text-primary mb-1">"Terms of Service"</h1>
            <p class="text-label-md text-on-surface-variant mb-xl">"Last updated: 2026-08-02"</p>

            <p class="mb-lg">"By using NotionCal (\"the Service\"), you agree to these terms."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"The Service"</h2>
            <p class="mb-lg">"The Service connects to a Notion workspace you authorize, and exposes the database(s) you choose as a CalDAV feed and a browser-based calendar view. There is no guaranteed uptime or support response time."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Price and billing"</h2>
            <p class="mb-lg">"The first six months are free and need no card. After that the Service costs $1 per year, billed annually and renewing automatically until you cancel. Paddle.com is the merchant of record: it takes the payment, issues the invoice, and handles tax."</p>
            <p class="mb-lg">"You can cancel whenever you like, and what happens when you do — along with when a payment can be refunded — is set out in our "<A href="/refund" attr:class="text-secondary underline">"Refund Policy"</A>"."</p>

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

#[component]
pub fn RefundRoutePage() -> impl IntoView {
    view! {
        <LegalPage
            title="Refund Policy"
            description="When a NotionCal payment can be refunded, how to cancel a subscription, and what happens to your data afterwards."
            path="/refund"
        >
            <h1 class="text-h1 font-semibold text-primary mb-1">"Refund Policy"</h1>
            <p class="text-label-md text-on-surface-variant mb-xl">"Last updated: 2026-10-31"</p>

            <p class="mb-lg">"NotionCal is free for its first six months and asks for no card during that time. After that it costs $1 per year. This page says when that payment can be given back and how to stop it recurring."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Refunds"</h2>
            <p class="mb-lg">"Write to us within 30 days of a charge and we will refund it in full. We do not ask why. The charge is one dollar; a policy that made you argue for it would cost us both more than the dollar."</p>
            <p class="mb-lg">"After 30 days we will still look at it — a renewal you did not expect, a year you could not use because the Service was broken — but outside that window a refund is a decision rather than a right."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Cancelling"</h2>
            <p class="mb-lg">"You can cancel at any time, and cancelling is not the same as a refund. It stops the next renewal; the year you have already paid for runs to its end, and the Service keeps working until then. We do not refund part of a year on cancellation alone — ask for a refund instead if that is what you want, and if you are inside the 30 days you will get the whole dollar back."</p>
            <p class="mb-lg">"There is no self-service cancel button yet. To cancel, email the address below from the account you signed up with, or use the manage-subscription link in the receipt Paddle emailed you. We will confirm by reply."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"How to ask"</h2>
            <p class="mb-lg">"Email "<a class="text-secondary underline" href="mailto:huyvu8051@gmail.com">"huyvu8051@gmail.com"</a>" from your account address, and say whether you want to cancel, to be refunded, or both. One person reads that inbox, so expect a reply within a few business days rather than within the hour."</p>
            <p class="mb-lg">"Because Paddle.com is the merchant of record, the money itself is returned by Paddle to the card or account you paid with. That usually lands within 5-10 business days once the refund is issued, depending on your bank. You can also contact Paddle directly through "<a class="text-secondary underline" href="https://paddle.net">"paddle.net"</a>", which is where your receipt comes from."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"Your data afterwards"</h2>
            <p class="mb-lg">"A refund or a cancellation does not delete your account by itself — your calendars and connections stay as they are, and the Service falls back to the free limits. If you want everything removed, say so in the same email and we will delete the account along with its connections, calendars, and credentials, as described in our "<A href="/privacy" attr:class="text-secondary underline">"Privacy Policy"</A>"."</p>
            <p class="mb-lg">"Revoking the Service's access from Notion's own \"Connections\" settings stops it reading your workspace immediately, whether or not you have asked us for anything."</p>

            <h2 class="text-h2 text-primary mt-xl mb-sm">"If the Service shuts down"</h2>
            <p>"If we discontinue NotionCal, we will refund the unused part of any year already paid for, without being asked."</p>
        </LegalPage>
    }
}
