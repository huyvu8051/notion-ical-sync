pub async fn landing_page(
    lang: crate::i18n::Lang,
    request: axum::extract::Request,
) -> axum::response::Response {
    let data = match lang {
        crate::i18n::Lang::En => app::landing::en_data(),
        crate::i18n::Lang::Vi => app::landing::vi_data(),
    };
    let handler = leptos_axum::render_app_to_stream(move || {
        let data = data.clone();
        leptos::view! { <app::landing::LandingShell data=data/> }
    });
    handler(request).await
}
