# Notion Marketplace submission — handoff

Form URL (must be logged in as huyvu8051@gmail.com, the integration owner):
https://app.notion.com/profile/connections/form/new?integrationId=3afd872b-594c-819a-8589-0037d3c3b8b3

The text fields are already saved on the form as of 2026-08-04. If it's ever reset
(there was one accidental "Discard draft" incident before), re-enter exactly this:

| Field | Value |
|---|---|
| Connection name | `NotionCal` |
| Tagline | `Turn any Notion database into a two-way calendar` |
| Category | `Productivity` |
| Description | See below |
| How to get started | See below |
| Installation URL | `https://notion-caldav.opendiy.vn/` |
| Company name | `OpenDIY` |
| Website | `https://notion-caldav.opendiy.vn/` |
| Email | `huyvu8051@gmail.com` |
| Privacy policy URL | `https://notion-caldav.opendiy.vn/privacy` |
| Terms of use URL | `https://notion-caldav.opendiy.vn/terms` |

**Description** (paste as 3 separate paragraphs, press Enter twice between each —
typing it as one block with embedded newlines has caused Notion's editor to scramble
paragraph order before):

> NotionCal turns any Notion database into a fully synced calendar. Add a Date property to your pages, connect your database, and every page becomes a calendar event automatically.
>
> View and manage events in a built in calendar webview, or subscribe via CalDAV to see them in Apple Calendar, Google Calendar, or any app that supports the CalDAV protocol. Changes sync both ways: edit a date in Notion and your calendar updates instantly, or drag an event in the calendar and Notion updates too.
>
> Real time updates are powered by Notion webhooks, so there is no waiting for a refresh. Great for teams and individuals who plan projects, content, or events in Notion but want a native calendar view too.

**How to get started** (one flowing paragraph — avoid leading digits like "1." at the
start of a line, that's triggered unwanted numbered-list auto-formatting before):

> Click Connect, sign in with Notion, and select the databases you want to sync. Add a Date property to each database if it does not already have one. You will get a calendar webview link and a CalDAV URL to add to Apple Calendar, Google Calendar, or any calendar app, ready in under a minute.

## Icon / Gallery images — known Notion-side bug

Uploading either field has repeatedly reported success (spinner completes, dialog
closes) but the image never actually persists — the form still flags it as missing,
which blocks "Submit for review" since Gallery images is required. Reported to Notion
support, ticket `PX3RNV-G4JNL` (case 5394363) — a screen recording is attached there:
https://github.com/huyvu8051/notioncal-marketplace-upload-bug

Notion support's latest suggestion (2026-08-05, agent Chaelim) was to try a smaller
file size. Pre-generated candidates are in `docs/assets/marketplace/` in this repo —
try smallest first:

- Icon: `icon_128.png` → `icon_256.png` → `icon_512.png` → `icon_1024.png`
- Gallery image: `gallery_1280x800.png` → `gallery_1600x1000.png` → `gallery_2048x1280.png`
  (2048×1280 is the dimension Notion's own dropzone hint asks for, but try smaller
  first per the support suggestion since that's the current working theory)

If a given size still doesn't persist after uploading, note which size(s) were tried
in the support email thread (reply-all, subject "Bug Report", ticket PX3RNV-G4JNL) so
there's a record of what's been ruled out.

Once an image *does* stick, `Submit for review` should go from disabled to enabled —
do **not** click it without checking with Huy first (review submission is effectively
a one-way action once Notion starts reviewing).
