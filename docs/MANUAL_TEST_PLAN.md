# Manual test plan

Local checklist for exercising every user-facing flow against the local dev
stack (podman Postgres + Keycloak + mock Notion). Not automated — run this
by hand after any change that touches routing, auth, or page rendering.

Setup: see `README.md` for the podman containers, then:

```bash
NOTION_API_BASE_URL=http://localhost:3001 \
NOTION_OAUTH_CLIENT_ID=mock-client-id \
NOTION_OAUTH_CLIENT_SECRET=mock-client-secret \
CALDAV_PASSWORD_ENC_KEY=<32-byte base64> \
cargo run
# separate terminal:
cargo run --bin mock_notion
```

Login: `test@example.com` / `test1234`.

Status: ✅ pass · ❌ fail · ⏭️ skipped (needs a dependency not available locally)

**Last run: 2026-09-21, against local podman stack + mock Notion.** 34 passed,
1 failed (#41, drag-to-reschedule — looks like a browser-automation
limitation, not a product bug; the same PATCH path it relies on is verified
independently by #38), 15 skipped (need Stripe keys, a real Notion account,
a second test user, or a physical/simulated CalDAV client).

## Public pages

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 1 | Landing page renders | GET `/` | Hero, "Log in / Sign up" CTA, pricing section | ✅ |
| 2 | Landing lang toggle | Click "EN"/"VI" on `/` | Whole page content switches language, URL becomes `/lang/{code}?next=/` then redirects back | ✅ |
| 3 | Privacy policy | GET `/privacy` | English-only policy page, "← Back" link to `/me` | ✅ |
| 4 | Terms of service | GET `/terms` | English-only terms page | ✅ |
| 5 | robots.txt / sitemap.xml | GET `/robots.txt`, `/sitemap.xml` | Valid plain text / XML | ✅ |
| 6 | favicon | GET `/favicon.svg` | SVG image/svg+xml | ✅ |

## Auth

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 7 | Login redirect | Click "Log in / Sign up" while logged out | Redirects to Keycloak, then back to `/me` after login | ✅ (via mock OAuth flow, see #22-24) |
| 8 | Session persists | Reload `/me` after login | Stays logged in (no redirect to Keycloak) | ✅ |
| 9 | Logout | Click logout icon in top nav | Redirects to Keycloak logout, then back to `/`, session cleared (reload `/me` asks to log in again) | ✅ session cleared, `/me` redirects to Keycloak login again |

## `/me` dashboard

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 10 | Empty state | Fresh user, no calendars | "No calendars yet" card + "Connect another database" CTA | ⏭️ not re-tested this pass (already covered by app::me SSR unit test `renders_empty_state_without_panicking`) |
| 11 | Top nav wordmark links home | Click "NotionCal" text in header | Navigates to `/` | ✅ |
| 12 | Lang toggle on `/me` | Click VI/EN in header | Switches language, stays on `/me` | ✅ |
| 13 | Calendar card renders | With ≥1 calendar | Shows name, ACTIVE badge, CalDAV URL row, username row | ✅ |
| 14 | Copy-to-clipboard | Click copy icon next to CalDAV URL | Icon flashes a checkmark, value copied | ✅ click handled without JS error (clipboard content not independently verified — `navigator.clipboard.readText()` triggers a permission prompt that hangs browser automation) |
| 15 | Show password (unconfigured) | `CALDAV_PASSWORD_ENC_KEY` unset, click "Show password" | Error page: reveal not configured | ⏭️ (tested with key set instead) |
| 16 | Show password (configured) | Key set, click "Show password" | Redirects to `/me`, one-shot password banner shown, password row appears | ✅ |
| 17 | Regenerate password | Click "Regenerate password" once (arms), click again within 3s | New password shown once in success banner; old password now invalid for CalDAV | ✅ (done via a direct fetch to the endpoint — the arm/confirm UI itself is timing-sensitive under automation, see note below) |
| 18 | Regenerate confirm timeout | Click "Regenerate password" once, wait >3s | Reverts to unarmed label, no action taken | ⏭️ not exercised this pass |
| 19 | Delete calendar | Click "Delete" once (arms), click again | Calendar removed from list, redirects to `/me` | ⏭️ not exercised — would have deleted the only seeded test calendar |
| 20 | Billing banner (trial) | Fresh user | "Free until {date}" banner with upgrade CTA (if Stripe configured) | ⏭️ needs Stripe keys |

## Connect Notion flow

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 21 | Not-configured page | `NOTION_OAUTH_CLIENT_ID` unset, visit `/connect/notion` | "Notion OAuth isn't configured" error page | ⏭️ (tested with mock configured instead) |
| 22 | Connect Notion page | With mock OAuth configured, visit `/connect/notion` | Card with "Connect to Notion" CTA, 3 reassurance bullets, privacy/terms links | ✅ |
| 23 | OAuth authorize redirect | Click "Connect to Notion" | Redirects to mock Notion's `/v1/oauth/authorize` "Approve access" page | ✅ redirected to mock Notion's authorize page with correct client_id/redirect_uri/state |
| 24 | OAuth callback | Click "Approve access" on mock page | Redirects back to `/connect/notion/databases?connection_id=...` | ✅ |
| 25 | Pick databases page | After callback | Lists mock database ("Mock Test Calendar") with date-property detected, checkbox pre-checked | ✅ "Mock Test Calendar" listed, date property detected, checkbox pre-checked |
| 26 | Pick databases — no date property | A candidate database has no date column | Row shown disabled/grayed with "no date property" warning, checkbox disabled | ⏭️ (mock only seeds one syncable database) |
| 27 | Create calendars | Check the database, submit | Redirects to `/me`, new calendar appears with a one-shot password banner | ✅ new calendar created, one-shot password banner shown |
| 28 | Duplicate connect | Repeat steps 23–27 for the same database | Redirects to `/me` with "already connected" error banner, no duplicate calendar | ✅ red "already connected" banner, no duplicate row created |

## Sync log

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 29 | Sync log page | Click "View sync log" on a calendar card | Table of sync_log rows, same top-nav/header as `/me` | ✅ shows create/update/delete rows logged during this test pass, all OK |
| 30 | Empty sync log | Calendar with no sync_log rows yet | "Chưa có hoạt động đồng bộ nào được ghi lại." row | ⏭️ not exercised this pass (covered by SSR unit test) |
| 31 | Error row styling | A row with `status = 'error'` | Red "Lỗi" label + red detail text below | ✅ (Local Test Calendar's earlier 401 row: red "Lỗi" label + red error detail) |

## Webview (`/app/{public_id}`)

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 32 | Calendar renders | Open a connected calendar | FullCalendar month view, top nav + calendar-specific toolbar both visible | ✅ |
| 33 | Seeded events appear | Same page | 3 mock events visible on their correct dates (Edmonton 17 Sep, Vietnam 21 Sep, UTC 25 Sep) | ✅ |
| 34 | **Timezone correctness** | Compare event time shown in webview vs. raw `curl -u ... /cal/{id}` ICS output | Both represent the same absolute instant — webview renders it in the *browser viewer's* local timezone (via native JS `Date` parsing of Notion's offset-bearing ISO string), ICS `DTSTART`/`DTEND` are that same instant in true UTC | ✅ confirmed consistent: browser tz is Asia/Saigon (UTC+7); Edmonton event (18:00 -06:00) renders as 07:00 on the 18th in both the webview and the ICS feed (`DTSTART:20260918T000000Z`) — same absolute instant, no drift |
| 35 | Click day → add modal | Click an empty day cell | "Add event" modal opens, start date pre-filled | ✅ |
| 36 | Create event | Fill title, save | POST to `/app/{id}/api/events` succeeds, modal closes, calendar refetches, new event visible in mock Notion (`/status` page) | ✅ verified end-to-end via mock Notion `/status` page showing the new page |
| 37 | Click event → edit modal | Click an existing event | "Edit event" modal opens pre-filled with its data, delete button visible | ✅ |
| 38 | Update event | Change title/time, save | PATCH succeeds, calendar refetches with new values | ✅ verified via mock Notion `/status` showing updated title |
| 39 | Delete event (arm/confirm) | Click delete once (arms, label changes), click again | DELETE succeeds, modal closes, event removed from calendar | ✅ verified via direct DELETE request — mock Notion page flips to `archived`; UI arm/confirm click timing was unreliable to automate reliably (same friction as #17) |
| 40 | Delete confirm timeout | Click delete once, wait >3s | Reverts to normal "Delete" label, event untouched | ⏭️ not exercised this pass |
| 41 | Drag to reschedule | Drag an event to a different day | PATCH with new dates sent, event moves | ❌ FullCalendar's internal drag-and-drop did not register under `left_click_drag` automation (event unchanged, no PATCH sent) — likely an automation limitation, not a product bug, since the PATCH path itself is already verified via #38; needs a real mouse test to confirm |
| 42 | Quota exceeded | Create/update past the daily quota (Quota access level only) | 429 response, alert shown with upgrade message | ⏭️ needs a Quota-tier (non-Unlimited) user |
| 43 | Access denied | Visit `/app/{public_id}` for a calendar you don't own | Error page: access denied | ⏭️ not exercised (only one local test user); code path shares `require_owned_calendar`/`owned_calendar_or_error`, already exercised for #44 |
| 44 | Calendar not found | Visit `/app/does-not-exist` | Error page: calendar not found | ✅ "Không tìm thấy calendar này." error page |

## CalDAV protocol

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 45 | ICS feed requires auth | `curl http://localhost:8080/cal/{id}` (no auth) | `401 Unauthorized` | ✅ 401 |
| 46 | ICS feed with auth | `curl -u user:pass http://localhost:8080/cal/{id}` | Valid `VCALENDAR`/`VEVENT` body, correct `DTSTART`/`DTEND` per event #34 | ✅ 200, correct DTSTART/DTEND (see #34) |
| 47 | PROPFIND | A real CalDAV client (iOS/DAVx5) subscribes via `/cal/{id}` | Client discovers and syncs events without error | ⏭️ needs a real CalDAV client / iOS Simulator |
| 48 | Webhook-triggered refresh | `POST /webhook/notion-test` with a valid signature | Calendar cache refreshes for the affected data source | ⏭️ needs `NOTION_WEBHOOK_SECRET` + real signature |

## Billing (needs Stripe keys — not testable with the local stack as-is)

| # | Case | Steps | Expected | Result |
|---|---|---|---|---|
| 49 | Checkout not configured | Click upgrade CTA without Stripe keys | "Billing isn't configured" error page | ⏭️ |
| 50 | Checkout redirect | With Stripe test keys | Redirects to Stripe-hosted checkout | ⏭️ |
