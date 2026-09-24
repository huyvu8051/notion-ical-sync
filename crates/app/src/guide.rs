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
                    <p class="mb-md">"Tap \"Download for iOS (2-way sync)\" right after generating your password. Safari asks to download a configuration profile — tap \"Allow\"."</p>
                    <img src="/static/guide/05-ios-download-prompt.jpg" alt="Safari asking to allow the configuration profile download" class="w-full rounded-lg border border-outline-variant mb-md"/>

                    <p class="mb-md">"iOS switches to Settings and shows \"Profile Downloaded\" — tap it, then tap the profile again to review it."</p>
                    <img src="/static/guide/06-ios-profile-downloaded.jpg" alt="Settings showing the Profile Downloaded row" class="w-full rounded-lg border border-outline-variant mb-md"/>

                    <p class="mb-md">"Tap \"Install\" in the top-right corner. You'll see an \"Unsigned Profile\" warning — that's expected, not an error. NotionCal doesn't pay for a code-signing certificate, and neither do most small CalDAV services; the warning doesn't stop the account from working. Tap \"Install\" again to confirm."</p>
                    <img src="/static/guide/07-ios-unsigned-warning.jpg" alt="iOS unsigned profile warning, safe to install anyway" class="w-full rounded-lg border border-outline-variant mb-md"/>

                    <p class="mb-md">"Done — the profile installs and every calendar you own now appears in Apple Calendar, fully two-way: events you create or edit there sync back to Notion, and vice versa."</p>
                    <img src="/static/guide/08-ios-profile-installed.jpg" alt="Profile Installed confirmation screen" class="w-full rounded-lg border border-outline-variant"/>

                    <p class="mb-md mt-lg">"Prefer to type it in yourself? Go to "<strong>"Settings → Apps → Calendar → Accounts → Add Account → Other → Add CalDAV Account"</strong>" and enter the CalDAV URL, username, and password from step 3 (leave off "<code>"https://"</code>" in the Server field — iOS adds it automatically)."</p>
                    <img src="/static/guide/10-ios-manual-caldav.jpg" alt="iOS manual CalDAV account form with server, username, password and description filled in" class="w-full rounded-lg border border-outline-variant"/>
                </GuideStep>

                <GuideStep number=5 title="Android: DAVx5">
                    <p class="mb-md">"Android has no OS-level equivalent to the iOS profile above — the standard way to get two-way CalDAV sync is the "<a class="text-secondary underline" href="https://f-droid.org/packages/at.bitfire.davdroid/" target="_blank">"DAVx5"</a>" app. Install it, tap \"Add account\", then either:"</p>
                    <ul class="list-disc pl-lg space-y-xs mb-md">
                        <li>"Open your phone's Camera app (or any QR scanner) and point it at the "<strong>"Android (DAVx5)"</strong>" code from step 3 — it'll offer to open DAVx5 with the server and username already filled in; you still type the password yourself."</li>
                        <li>"Or in DAVx5 choose \"Login with URL and user name\" and type the three fields from step 3 in by hand."</li>
                    </ul>
                    <img src="/static/guide/09-android-davx5-prefilled.jpg" alt="DAVx5 login screen with server and username pre-filled after scanning the QR code" class="w-full rounded-lg border border-outline-variant mb-md"/>
                    <p class="text-on-surface-variant text-body-md">"Same result as iOS: every calendar you own shows up, fully two-way."</p>
                </GuideStep>

                <GuideStep number=6 title="Other apps: Google Calendar, Thunderbird, or any CalDAV client">
                    <p class="mb-md">"Copy the three fields from step 3 — CalDAV URL, username, password — into your app's own \"Add CalDAV account\" or \"Subscribe to calendar\" screen:"</p>
                    <ul class="list-disc pl-lg space-y-xs">
                        <li><strong>"Two-way sync "</strong>"(create/edit/delete from your calendar app): use an app that supports adding a full CalDAV account, e.g. Thunderbird."</li>
                        <li><strong>"Read-only: "</strong>"most calendar apps also let you \"subscribe\" to a calendar by URL — paste a single calendar's URL there for a live, view-only feed."</li>
                    </ul>
                </GuideStep>

                <GuideStep number=7 title="Or just use the built-in calendar">
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
