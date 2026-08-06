//! Static Privacy Policy / Terms of Service pages — required both for the
//! Notion OAuth consent screen and for eventual Notion Marketplace
//! submission (Phase 6 of the plan). Plain unauthenticated routes, no DB
//! access, so they render even if something else is broken.

use axum::response::{Html, IntoResponse};

const LEGAL_STYLE: &str = r#"
<style>
  * { box-sizing: border-box; }
  body { font-family: -apple-system, sans-serif; max-width: 720px; margin: 3rem auto; padding: 0 1.25rem; line-height: 1.6; color: #1a1a1a; }
  h1 { margin-bottom: 0.25rem; }
  .updated { color: #888; font-size: 0.85rem; margin-bottom: 2rem; }
  h2 { margin-top: 2rem; font-size: 1.15rem; }
  ul { padding-left: 1.25rem; }
  li { margin: 0.35rem 0; }
  a { color: #2563eb; }
  .top-nav { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
  .top-nav a.back { font-size: 0.85rem; color: #666; text-decoration: none; }
</style>
"#;

fn page(title: &str, back_label: &str, body: &str) -> Html<String> {
    Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — NotionCal</title>{LEGAL_STYLE}</head>
<body>
<div class="top-nav"><strong>NotionCal</strong><a class="back" href="/me">{back_label}</a></div>
{body}
</body></html>"#
    ))
}

pub async fn privacy_policy_page() -> impl IntoResponse {
    page(
        "Privacy Policy",
        "← Back",
        r#"
<h1>Privacy Policy</h1>
<p class="updated">Last updated: 2026-08-02</p>

<p>NotionCal ("the Service") turns a Notion database into a CalDAV feed and
a browser calendar view. This page explains what data we collect and how we use it.</p>

<h2>What we collect</h2>
<ul>
  <li><strong>Account:</strong> the email address from your login (via our
  self-hosted Keycloak identity provider) — used only to identify your account.</li>
  <li><strong>Notion access:</strong> when you connect your Notion workspace, we
  store the OAuth access token Notion issues us, along with your workspace id/name
  and integration bot id. This token is what lets the Service read and write the
  specific pages you granted access to.</li>
  <li><strong>Calendar configuration:</strong> for each Notion database you choose
  to sync, we store its database id, the date property used for scheduling, and a
  display name.</li>
  <li><strong>CalDAV credentials:</strong> we generate a random username/password
  per calendar so you can subscribe from Apple/Google Calendar or any CalDAV
  client. Only a salted hash (Argon2) of the password is stored — the plaintext is
  shown to you once, at creation time, and never again.</li>
</ul>

<h2>What we don't collect</h2>
<ul>
  <li>No payment or billing information — the Service is free.</li>
  <li>No analytics or advertising trackers.</li>
  <li>No data is sold or shared with third parties for marketing.</li>
</ul>

<h2>How we use it</h2>
<p>Solely to operate the Service: fetching events from your Notion database,
converting them to CalDAV/iCalendar format, keeping them in sync (via periodic
polling and Notion's webhook events), and rendering the calendar webview so you
can view and edit events yourself.</p>

<h2>Where it's stored</h2>
<p>All data lives in a private PostgreSQL database we operate, not exposed to the
public internet, reachable only by the Service itself. We do not use third-party
data processors beyond Notion's own API (needed to read/write your workspace) and
the infrastructure hosting our servers.</p>

<h2>Your controls</h2>
<ul>
  <li>You can revoke the Service's access at any time from Notion's own
  "Connections" settings in your workspace — this immediately invalidates the
  access token we hold.</li>
  <li>To delete your account and all associated data (connections, calendars,
  credentials), email us at the address below.</li>
</ul>

<h2>Contact</h2>
<p>Questions about this policy: <a href="mailto:huyvu8051@gmail.com">huyvu8051@gmail.com</a></p>
"#,
    )
}

pub async fn terms_of_service_page() -> impl IntoResponse {
    page(
        "Terms of Service",
        "← Back",
        r#"
<h1>Terms of Service</h1>
<p class="updated">Last updated: 2026-08-02</p>

<p>By using NotionCal ("the Service"), you agree to these terms.</p>

<h2>The Service</h2>
<p>The Service connects to a Notion workspace you authorize, and exposes the
database(s) you choose as a CalDAV feed and a browser-based calendar view. It is
provided free of charge, with no guaranteed uptime or support response time.</p>

<h2>Your responsibilities</h2>
<ul>
  <li>You're responsible for the content of the Notion pages you connect, and for
  keeping your CalDAV credentials confidential.</li>
  <li>Don't use the Service to store or distribute illegal content, or in a way
  that places excessive load on it (e.g. automated scraping outside normal
  calendar-client sync behavior).</li>
  <li>You must have the right to grant the Service access to any Notion workspace
  you connect.</li>
</ul>

<h2>No warranty</h2>
<p>The Service is provided "as is," without warranty of any kind. We don't
guarantee it will be uninterrupted, error-free, or that data will never be lost —
Notion remains the source of truth for your data, and we recommend not relying on
the Service as your only backup of important events.</p>

<h2>Termination</h2>
<p>We may suspend or terminate access to the Service for any account found abusing
it (as described above), or discontinue the Service entirely. You may stop using
the Service and revoke its Notion access at any time.</p>

<h2>Changes</h2>
<p>We may update these terms as the Service evolves; continued use after a change
means you accept the updated terms.</p>

<h2>Governing law</h2>
<p>These terms are governed by the laws of Vietnam.</p>

<h2>Contact</h2>
<p><a href="mailto:huyvu8051@gmail.com">huyvu8051@gmail.com</a></p>
"#,
    )
}

pub async fn robots_txt() -> impl IntoResponse {
    let body = "User-agent: *\nAllow: /\nSitemap: https://notion-caldav.opendiy.vn/sitemap.xml\n";
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain")],
        body.to_string(),
    )
}

pub async fn sitemap_xml() -> impl IntoResponse {
    let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://notion-caldav.opendiy.vn/</loc>
    <changefreq>monthly</changefreq>
    <priority>1.0</priority>
  </url>
  <url>
    <loc>https://notion-caldav.opendiy.vn/privacy</loc>
    <changefreq>monthly</changefreq>
    <priority>0.5</priority>
  </url>
  <url>
    <loc>https://notion-caldav.opendiy.vn/terms</loc>
    <changefreq>monthly</changefreq>
    <priority>0.5</priority>
  </url>
</urlset>"#;
    (
        [(axum::http::header::CONTENT_TYPE, "application/xml")],
        body.to_string(),
    )
}
