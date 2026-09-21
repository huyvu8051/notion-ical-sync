use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectNotionPageData {
    pub html_lang: String,
    pub title: String,
    pub top_nav_html: String,
    pub heading: String,
    pub body: String,
    pub connect_cta: String,
    pub bullet_read_write: String,
    pub bullet_disconnect: String,
    pub bullet_no_sharing: String,
    pub privacy_link: String,
    pub terms_link: String,
}

use crate::page_shell::{GOOGLE_FONTS_HREF, ONBOARDING_HEAD_STYLE};

struct ConnectNotionLabels {
    title: &'static str,
    heading: &'static str,
    body: &'static str,
    connect_cta: &'static str,
    bullet_read_write: &'static str,
    bullet_disconnect: &'static str,
    bullet_no_sharing: &'static str,
    privacy_link: &'static str,
    terms_link: &'static str,
}

const CONNECT_NOTION_LABELS_VI: ConnectNotionLabels = ConnectNotionLabels {
    title: "Kết nối Notion",
    heading: "Kết nối không gian làm việc Notion của bạn",
    body: "Chúng tôi cần quyền truy cập vào không gian làm việc Notion của bạn để tìm và đồng bộ hóa các cơ sở dữ liệu bạn chọn. Bạn sẽ chọn chính xác trang nào cần chia sẻ ở bước tiếp theo trên Notion.",
    connect_cta: "Kết nối với Notion",
    bullet_read_write: "Chỉ đọc và ghi vào các trang bạn cho phép",
    bullet_disconnect: "Có thể ngắt kết nối bất cứ lúc nào",
    bullet_no_sharing: "Không bao giờ chia sẻ dữ liệu của bạn với bên thứ ba",
    privacy_link: "Chính sách bảo mật",
    terms_link: "Điều khoản dịch vụ",
};

const CONNECT_NOTION_LABELS_EN: ConnectNotionLabels = ConnectNotionLabels {
    title: "Connect Notion",
    heading: "Connect your Notion workspace",
    body: "We need access to your Notion workspace to find and sync the databases you choose. You'll pick exactly which pages to share in the next step, on Notion.",
    connect_cta: "Connect to Notion",
    bullet_read_write: "Only reads and writes the pages you allow",
    bullet_disconnect: "Disconnect anytime",
    bullet_no_sharing: "Never shares your data with third parties",
    privacy_link: "Privacy Policy",
    terms_link: "Terms of Service",
};

#[component]
pub fn ConnectNotionRoutePage() -> impl IntoView {
    let lang = crate::page_shell::detect_lang();
    let l = if lang == "en" {
        &CONNECT_NOTION_LABELS_EN
    } else {
        &CONNECT_NOTION_LABELS_VI
    };
    #[cfg(feature = "ssr")]
    let email = crate::page_shell::current_user_email();
    #[cfg(not(feature = "ssr"))]
    let email = String::new();
    let data = ConnectNotionPageData {
        html_lang: lang.to_string(),
        title: l.title.to_string(),
        top_nav_html: crate::page_shell::top_nav_html(&email, lang, "/connect/notion"),
        heading: l.heading.to_string(),
        body: l.body.to_string(),
        connect_cta: l.connect_cta.to_string(),
        bullet_read_write: l.bullet_read_write.to_string(),
        bullet_disconnect: l.bullet_disconnect.to_string(),
        bullet_no_sharing: l.bullet_no_sharing.to_string(),
        privacy_link: l.privacy_link.to_string(),
        terms_link: l.terms_link.to_string(),
    };
    view! {
        <leptos_meta::Html attr:lang=data.html_lang.clone()/>
        <leptos_meta::Title text=format!("{} — NotionCal", data.title)/>
        <leptos_meta::Link rel="stylesheet" href="/assets/style-auth-a.css"/>
        <leptos_meta::Link href=GOOGLE_FONTS_HREF rel="stylesheet"/>
        <leptos_meta::Style>{ONBOARDING_HEAD_STYLE}</leptos_meta::Style>
        <ConnectNotionPage data=data/>
    }
}

#[component]
pub fn ConnectNotionPage(data: ConnectNotionPageData) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    crate::page_shell::install_client_timezone_label();

    let body_html = format!(
        r##"{top_nav}
<main class="flex-grow flex items-center justify-center px-margin-mobile md:px-margin-desktop">
<div class="w-full max-w-[480px] bg-surface-container-lowest border border-outline-variant p-xl rounded-lg card-shadow">
<div class="flex justify-center items-center gap-md mb-lg">
<div class="w-12 h-12 flex items-center justify-center bg-surface-container border border-outline-variant rounded-xl">
<span class="material-symbols-outlined text-[28px]">link</span>
</div>
<div class="w-2 h-[1px] bg-outline-variant"></div>
<div class="w-12 h-12 flex items-center justify-center bg-primary rounded-xl">
<svg class="w-7 h-7 text-white fill-current" viewBox="0 0 24 24"><path d="M4.459 4.208c.673-.51 1.258-.69 2.067-.69h11.974c.421 0 .762.341.762.762v15.44c0 .421-.341.762-.762.762H5.539c-.588 0-1.026-.411-1.127-1.002L3.13 10.985C3.01 10.378 3.167 9.754 3.56 9.27L4.459 4.208zM17.15 17.61V6.63h-.04l-3.32 4.45h-.04V6.63h-1.28v10.98h.04l3.32-4.44h.04v4.44h1.28z"></path></svg>
</div>
</div>
<div class="text-center mb-xl">
<h1 class="text-h1 mb-sm tracking-tight text-primary">{heading}</h1>
<p class="text-body-md text-on-surface-variant leading-relaxed">{body}</p>
</div>
<a class="w-full h-12 bg-primary text-white text-label-md flex items-center justify-center gap-sm rounded-lg hover:bg-zinc-800 transition-all active:scale-[0.98] mb-lg" href="/connect/notion/start">
<svg class="w-5 h-5 fill-current" viewBox="0 0 24 24"><path d="M4.459 4.208c.673-.51 1.258-.69 2.067-.69h11.974c.421 0 .762.341.762.762v15.44c0 .421-.341.762-.762.762H5.539c-.588 0-1.026-.411-1.127-1.002L3.13 10.985C3.01 10.378 3.167 9.754 3.56 9.27L4.459 4.208zM17.15 17.61V6.63h-.04l-3.32 4.45h-.04V6.63h-1.28v10.98h.04l3.32-4.44h.04v4.44h1.28z"></path></svg>
{connect_cta}
</a>
<div class="border-t border-outline-variant pt-lg space-y-md">
<div class="flex items-start gap-md">
<span class="material-symbols-outlined text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">check_circle</span>
<span class="text-body-md text-on-surface-variant">{bullet_read_write}</span>
</div>
<div class="flex items-start gap-md">
<span class="material-symbols-outlined text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">check_circle</span>
<span class="text-body-md text-on-surface-variant">{bullet_disconnect}</span>
</div>
<div class="flex items-start gap-md">
<span class="material-symbols-outlined text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">check_circle</span>
<span class="text-body-md text-on-surface-variant">{bullet_no_sharing}</span>
</div>
</div>
<div class="mt-xl text-center">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors underline underline-offset-4" href="/privacy">{privacy_link}</a>
<span class="text-outline-variant mx-2">·</span>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors underline underline-offset-4" href="/terms">{terms_link}</a>
</div>
</div>
</main>"##,
        top_nav = data.top_nav_html,
        heading = data.heading,
        body = data.body,
        connect_cta = data.connect_cta,
        bullet_read_write = data.bullet_read_write,
        bullet_disconnect = data.bullet_disconnect,
        bullet_no_sharing = data.bullet_no_sharing,
        privacy_link = data.privacy_link,
        terms_link = data.terms_link,
    );

    view! {
        <div id="connect-notion-root" inner_html=body_html></div>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn sample_data() -> ConnectNotionPageData {
        ConnectNotionPageData {
            html_lang: "vi".to_string(),
            title: "Kết nối Notion".to_string(),
            top_nav_html: "<header>nav</header>".to_string(),
            heading: "Kết nối không gian làm việc Notion của bạn".to_string(),
            body: "Chúng tôi cần quyền truy cập...".to_string(),
            connect_cta: "Kết nối với Notion".to_string(),
            bullet_read_write: "Chỉ đọc và ghi vào các trang bạn cho phép".to_string(),
            bullet_disconnect: "Có thể ngắt kết nối bất cứ lúc nào".to_string(),
            bullet_no_sharing: "Không bao giờ chia sẻ dữ liệu của bạn với bên thứ ba".to_string(),
            privacy_link: "Chính sách bảo mật".to_string(),
            terms_link: "Điều khoản dịch vụ".to_string(),
        }
    }

    #[test]
    fn renders_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = view! { <ConnectNotionPage data=sample_data()/> }.to_html();
        assert!(html.contains("Kết nối với Notion"));
        assert!(html.contains("/connect/notion/start"));
    }
}
