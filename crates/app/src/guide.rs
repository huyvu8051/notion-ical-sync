use leptos::prelude::*;
use leptos_router::components::A;

#[component]
fn GuideStep(number: i32, title: &'static str, children: Children) -> impl IntoView {
    view! {
        <section class="space-y-sm">
            <h2 class="text-h2 text-primary flex items-center gap-sm">
                <span class="inline-flex items-center justify-center w-7 h-7 rounded-full bg-primary text-on-primary text-label-md font-semibold shrink-0">{number}</span>
                {title}
            </h2>
            {children()}
        </section>
    }
}

#[component]
pub fn GuideRoutePage() -> impl IntoView {
    view! {
        <leptos_meta::Html attr:lang="en"/>
        <leptos_meta::Title text="Setup Guide — NotionCal"/>
        <div id="guide-root" class="bg-background text-on-surface min-h-screen">
            <main class="max-w-[720px] mx-auto px-margin-mobile md:px-margin-desktop pt-[64px] pb-lg space-y-xl">
                <div>
                    <h1 class="text-h1 font-semibold text-primary mb-1">"Setup Guide"</h1>
                    <p class="text-on-surface-variant text-body-md">"How to connect a Notion database and sync it to your calendar app, step by step."</p>
                </div>

                <GuideStep number=1 title="Connect your Notion workspace">
                    <p class="mb-md">"From "<A href="/me" attr:class="text-secondary underline">"Your calendars"</A>", click \"Connect another database\" and authorize NotionCal to access your workspace. You pick exactly which database to share on Notion's own page — nothing else is touched."</p>
                    <img src="/static/guide/01-connect-notion.jpg" alt="Connect your Notion workspace screen" class="w-full rounded-lg border border-outline-variant"/>
                </GuideStep>

                <GuideStep number=2 title="Your calendars dashboard">
                    <p class="mb-md">"Each Notion database you pick shows up as its own calendar here, alongside one shared \"Account-wide CalDAV access\" credential that gives a calendar app access to every calendar you own at once."</p>
                    <img src="/static/guide/02-your-calendars.jpg" alt="Your calendars dashboard" class="w-full rounded-lg border border-outline-variant"/>
                </GuideStep>

                <GuideStep number=3 title="Get your CalDAV credentials">
                    <p class="mb-md">"Click \"Regenerate\" (or \"Generate account-wide access\" the first time) to reveal a CalDAV URL, username, and password. The password is shown only this once — copy it now, or use the iOS download button below right away."</p>
                    <img src="/static/guide/03-account-caldav.png" alt="Account-wide CalDAV credentials revealed, with a Download for iOS link" class="w-full rounded-lg border border-outline-variant"/>
                </GuideStep>

                <GuideStep number=4 title="iPhone or iPad: one-tap setup">
                    <p class="mb-md">"Tap \"Download for iOS (2-way sync)\" right after generating your password. This downloads a small configuration file:"</p>
                    <ol class="list-decimal pl-lg space-y-xs mb-md">
                        <li>"Open the downloaded "<code class="font-code text-code bg-surface-container-low px-1 rounded">"notioncal.mobileconfig"</code>" file (from the Files app or your Downloads)."</li>
                        <li>"iOS switches to Settings and shows \"Profile Downloaded\" — tap it."</li>
                        <li>"Tap "<strong>"Install"</strong>" in the top-right corner, enter your passcode if asked, then tap "<strong>"Install"</strong>" once more to confirm."</li>
                    </ol>
                    <p class="text-on-surface-variant text-body-md">"That's it — every calendar you own now appears in Apple Calendar, fully two-way: events you create or edit there sync back to Notion, and vice versa."</p>
                </GuideStep>

                <GuideStep number=5 title="Other apps: Google Calendar, DAVx5, or any CalDAV client">
                    <p class="mb-md">"Copy the three fields from step 3 — CalDAV URL, username, password — into your app's own \"Add CalDAV account\" or \"Subscribe to calendar\" screen:"</p>
                    <ul class="list-disc pl-lg space-y-xs">
                        <li><strong>"Two-way sync "</strong>"(create/edit/delete from your calendar app): use an app that supports adding a full CalDAV account, e.g. DAVx5 on Android or Thunderbird."</li>
                        <li><strong>"Read-only: "</strong>"most calendar apps also let you \"subscribe\" to a calendar by URL — paste a single calendar's URL there for a live, view-only feed."</li>
                    </ul>
                </GuideStep>

                <GuideStep number=6 title="Or just use the built-in calendar">
                    <p class="mb-md">"No calendar app needed — click \"Open calendar\" on any calendar in your dashboard to view, create, and edit events directly in your browser."</p>
                    <img src="/static/guide/04-webview-calendar.jpg" alt="Built-in browser calendar view" class="w-full rounded-lg border border-outline-variant"/>
                </GuideStep>

                <div class="pt-md border-t border-outline-variant">
                    <A href="/me" attr:class="text-secondary underline">"Back to your calendars"</A>
                </div>
            </main>
            <div class="max-w-[1280px] mx-auto w-full px-margin-desktop">
                <crate::page_shell::PageFooter/>
            </div>
        </div>
    }
}
