use axum::response::IntoResponse;

pub async fn privacy_policy_page(request: axum::extract::Request) -> axum::response::Response {
    let handler = leptos_axum::render_app_to_stream(|| {
        leptos::view! { <app::legal::LegalShell data=app::legal::privacy_data()/> }
    });
    handler(request).await
}

pub async fn terms_of_service_page(request: axum::extract::Request) -> axum::response::Response {
    let handler = leptos_axum::render_app_to_stream(|| {
        leptos::view! { <app::legal::LegalShell data=app::legal::terms_data()/> }
    });
    handler(request).await
}

pub async fn robots_txt() -> impl IntoResponse {
    let body = "User-agent: *\nAllow: /\nSitemap: https://notion-caldav.opendiy.vn/sitemap.xml\n";
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain")],
        body.to_string(),
    )
}

const FAVICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<rect x="2" y="2" width="28" height="28" rx="6" fill="#fbf9f9"/>
<rect x="6" y="9" width="20" height="17" rx="2" fill="none" stroke="#000" stroke-width="2"/>
<line x1="6" y1="14" x2="26" y2="14" stroke="#000" stroke-width="2"/>
<line x1="11" y1="6" x2="11" y2="11" stroke="#000" stroke-width="2" stroke-linecap="round"/>
<line x1="21" y1="6" x2="21" y2="11" stroke="#000" stroke-width="2" stroke-linecap="round"/>
<circle cx="11" cy="19" r="1.4"/>
<circle cx="16" cy="19" r="1.4"/>
<circle cx="21" cy="19" r="1.4"/>
</svg>"##;

pub async fn favicon() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        FAVICON_SVG,
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
