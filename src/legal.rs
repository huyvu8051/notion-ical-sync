//! Privacy Policy / Terms of Service pages — required both for the Notion
//! OAuth consent screen and for eventual Notion Marketplace submission
//! (Phase 6 of the plan). Plain unauthenticated routes, no DB access, so
//! they render even if something else is broken.
//!
//! Phase 0 of the full Leptos SSR+CSR migration (see
//! `~/.claude/plans/mighty-scribbling-floyd.md`): the `<body>` content is a
//! real `app::PrivacyPage`/`app::TermsPage` Leptos component, rendered here
//! via SSR and hydrated client-side by `/pkg/app.js`. The outer
//! `<html><head>` shell stays hand-written, same as before.

use axum::response::{Html, IntoResponse};
use leptos::prelude::*;

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
  .helpful { margin-top: 3rem; padding-top: 1rem; border-top: 1px solid #eee; font-size: 0.9rem; color: #555; }
  .helpful button { margin-left: 0.5rem; cursor: pointer; }
</style>
"#;

fn page(title: &str, data_page: &str, body: String) -> Html<String> {
    Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — NotionCal</title>{LEGAL_STYLE}</head>
<body data-page="{data_page}">
{body}
<script type="module">
import init from '/pkg/app.js';
init();
</script>
</body></html>"#
    ))
}

pub async fn privacy_policy_page() -> impl IntoResponse {
    page(
        "Privacy Policy",
        "privacy",
        view! { <app::PrivacyPage /> }.to_html(),
    )
}

pub async fn terms_of_service_page() -> impl IntoResponse {
    page(
        "Terms of Service",
        "terms",
        view! { <app::TermsPage /> }.to_html(),
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
