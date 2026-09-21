use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct LandingPageData {
    pub html_lang: String,
    pub page_title: String,
    pub meta_description: String,
    pub twitter_description: String,
    pub og_locale: String,
    pub lang_toggle_href: String,
    pub lang_toggle_label: String,
    pub body_html: String,
}

const LANDING_HEAD_STYLE: &str = r#"
.material-symbols-outlined { font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24; vertical-align: middle; }
body { font-family: 'Inter', sans-serif; -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; }
.bento-grid { display: grid; grid-template-columns: repeat(12, 1fr); gap: 1rem; }
.hairline-border { border: 1px solid #E5E5E5; }
.hover-lift:hover { transform: translateY(-2px); transition: transform 0.2s ease-out; border-color: #D4D4D4; }
.glass-header { backdrop-filter: blur(8px); background: rgba(251, 249, 249, 0.85); }
"#;

use crate::page_shell::GOOGLE_FONTS_HREF;

const BING_VALIDATE: &str = "B2ADD65C06672433A78251607DBB1250";
const CANONICAL_URL: &str = "https://notion-caldav.opendiy.vn/";

const BODY_HTML_VI: &str = r##"
<header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
<div class="flex items-center gap-8">
<a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
<span class="material-symbols-outlined text-primary">calendar_month</span>
NotionCal
</a>
<nav class="hidden md:flex items-center gap-6">
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#how-it-works">Cách hoạt động</a>
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#pricing">Giá cả</a>
</nav>
</div>
<div class="flex items-center gap-4">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors px-2 py-2" href="/lang/en?next=/">EN</a>
<a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me">Đăng nhập / Đăng ký</a>
</div>
</div>
</header>
<main class="pt-[64px]">
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl md:py-[120px]">
<div class="grid grid-cols-1 lg:grid-cols-2 gap-xl items-center">
<div class="space-y-lg">
<div class="inline-flex items-center gap-2 px-3 py-1 bg-surface-container rounded border border-outline-variant text-on-surface-variant">
<span class="material-symbols-outlined text-[16px]">sync</span>
<span class="text-[12px] uppercase tracking-wider font-bold">Đồng bộ hai chiều</span>
</div>
<h1 class="text-[48px] md:text-[64px] leading-tight text-primary font-extrabold tracking-tight">
Biến cơ sở dữ liệu Notion thành lịch trực tuyến
</h1>
<p class="text-body-lg text-on-surface-variant max-w-[540px]">
Đồng bộ hóa Notion của bạn với Apple Calendar, Google Calendar hoặc bất kỳ ứng dụng CalDAV nào. Chỉnh sửa linh hoạt từ cả hai phía.
</p>
<div class="pt-sm">
<a class="bg-primary text-on-primary h-[48px] px-8 rounded-lg text-h3 inline-flex items-center gap-3 hover:opacity-90 transition-all active:scale-[0.98]" href="/me">
<span class="material-symbols-outlined" style="font-variation-settings: 'FILL' 1;">login</span>
Đăng nhập / Đăng ký
</a>
<p class="mt-4 text-label-md text-on-surface-variant flex items-center gap-2">
<span class="material-symbols-outlined text-[16px] text-secondary">verified</span>
6 tháng đầu miễn phí, không giới hạn. Không cần thẻ tín dụng.
</p>
</div>
</div>
<div class="relative group">
<div class="absolute -inset-4 bg-gradient-to-r from-secondary-container/10 to-primary/5 rounded-xl blur-2xl group-hover:opacity-75 transition-opacity"></div>
<div class="relative rounded-xl hairline-border overflow-hidden bg-white shadow-sm p-lg">
<div class="flex items-center justify-center h-64 bg-surface-container rounded-lg border border-dashed border-outline-variant">
<span class="material-symbols-outlined !text-[64px] text-outline opacity-40">calendar_month</span>
</div>
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="how-it-works">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">Quy trình đơn giản</h2>
<p class="text-body-md text-on-surface-variant mt-2">Bắt đầu đồng bộ hóa dữ liệu của bạn chỉ với 3 bước</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-3 gap-gutter">
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">link</span>
</div>
<h3 class="text-h3 text-primary mb-2">1. Kết nối Notion</h3>
<p class="text-body-md text-on-surface-variant">Kết nối không gian làm việc Notion của bạn một cách bảo mật thông qua OAuth.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">database</span>
</div>
<h3 class="text-h3 text-primary mb-2">2. Chọn cơ sở dữ liệu</h3>
<p class="text-body-md text-on-surface-variant">Chọn cơ sở dữ liệu bạn muốn đồng bộ. Yêu cầu có thuộc tính kiểu Date.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">event_available</span>
</div>
<h3 class="text-h3 text-primary mb-2">3. Đồng bộ lịch</h3>
<p class="text-body-md text-on-surface-variant">Đăng ký trên ứng dụng lịch của bạn (Apple, Google, Outlook) thông qua link CalDAV.</p>
</div>
</div>
</div>
</section>
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl">
<div class="bento-grid grid-rows-2">
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-white flex flex-col justify-between">
<div>
<h4 class="text-h2 text-primary mb-2">Đồng bộ hai chiều thời gian thực</h4>
<p class="text-body-md text-on-surface-variant">Thay đổi trên Notion sẽ cập nhật ngay lập tức trên Lịch của bạn và ngược lại. Không bao giờ bỏ lỡ một deadline nào.</p>
</div>
<div class="mt-xl h-32 bg-surface-container rounded-lg border border-dashed border-outline flex items-center justify-center">
<span class="material-symbols-outlined text-[48px] text-outline opacity-40">sync_alt</span>
</div>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-surface-container flex flex-col justify-center items-center text-center">
<span class="material-symbols-outlined text-[48px] mb-4 text-secondary">security</span>
<h4 class="text-h3 text-primary">Dữ liệu của bạn, do bạn kiểm soát</h4>
<p class="text-body-md text-on-surface-variant mt-2">Chỉ đọc/ghi vào các trang bạn cho phép. Có thể ngắt kết nối bất cứ lúc nào.</p>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-white">
<h4 class="text-h3 text-primary mb-2">Giá đơn giản</h4>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> 6 tháng đầu miễn phí, không giới hạn
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Sau đó chỉ $1/năm
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Không cần thẻ tín dụng để bắt đầu
</li>
</ul>
</div>
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-primary text-on-primary flex items-center justify-between">
<div>
<h4 class="text-h2 mb-1">Dành cho mọi ứng dụng lịch</h4>
<p class="text-label-md opacity-80 uppercase tracking-widest">Chuẩn giao thức CalDAV</p>
</div>
<div class="text-code bg-white/10 p-3 rounded hairline-border border-white/20">
notion-caldav.opendiy.vn/cal/...
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="pricing">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">Giá cả</h2>
<p class="text-body-md text-on-surface-variant mt-2">Dùng thử miễn phí 6 tháng, sau đó chỉ $1/năm</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-2 gap-gutter max-w-[720px] mx-auto">
<div class="bg-white p-lg hairline-border rounded-xl">
<h3 class="text-h2 text-primary mb-1">6 tháng đầu</h3>
<p class="text-display text-primary mb-2">Miễn phí</p>
<p class="text-body-md text-on-surface-variant mb-md">Dành cho mọi tài khoản mới, tính từ ngày đăng ký.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Không giới hạn số sự kiện
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Kết nối nhiều database
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Không cần thẻ tín dụng
</li>
</ul>
</div>
<div class="bg-primary text-on-primary p-lg hairline-border rounded-xl relative">
<div class="absolute -top-3 right-lg bg-secondary text-on-primary text-[11px] uppercase tracking-wider font-bold px-3 py-1 rounded-full">Sau 6 tháng</div>
<h3 class="text-h2 mb-1">Nâng cấp</h3>
<p class="text-display mb-2">$1<span class="text-h3 opacity-70">/năm</span></p>
<p class="text-body-md opacity-80 mb-md">Đăng ký sớm vẫn được hưởng trọn 6 tháng miễn phí — chỉ bắt đầu tính phí sau khi hết hạn.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Không giới hạn số sự kiện
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Thanh toán an toàn qua Stripe
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Huỷ bất cứ lúc nào
</li>
</ul>
</div>
</div>
<p class="text-center text-label-md text-on-surface-variant mt-lg">Hết 6 tháng miễn phí mà chưa đăng ký? Bạn vẫn dùng được, giới hạn 10 sự kiện mới/ngày.</p>
</div>
</section>
</main>
<footer class="border-t border-outline-variant bg-surface mt-xl">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop py-lg flex flex-col md:flex-row justify-between items-center gap-lg">
<div class="flex flex-col md:flex-row items-center gap-md">
<span class="text-h3 font-bold text-primary">NotionCal</span>
</div>
<div class="flex items-center gap-6">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/privacy">Chính sách bảo mật</a>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/terms">Điều khoản dịch vụ</a>
</div>
</div>
</footer>
"##;

const BODY_HTML_EN: &str = r##"
<header class="fixed top-0 left-0 right-0 z-50 glass-header border-b border-outline-variant">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop h-[64px] flex justify-between items-center">
<div class="flex items-center gap-8">
<a class="text-h2 font-bold text-primary flex items-center gap-2" href="/">
<span class="material-symbols-outlined text-primary">calendar_month</span>
NotionCal
</a>
<nav class="hidden md:flex items-center gap-6">
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#how-it-works">How it works</a>
<a class="text-body-md text-on-surface-variant hover:text-primary transition-colors" href="#pricing">Pricing</a>
</nav>
</div>
<div class="flex items-center gap-4">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors px-2 py-2" href="/lang/vi?next=/">VI</a>
<a class="bg-primary text-on-primary text-label-md px-4 py-2 rounded transition-transform active:scale-95 duration-100" href="/me">Log in / Sign up</a>
</div>
</div>
</header>
<main class="pt-[64px]">
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl md:py-[120px]">
<div class="grid grid-cols-1 lg:grid-cols-2 gap-xl items-center">
<div class="space-y-lg">
<div class="inline-flex items-center gap-2 px-3 py-1 bg-surface-container rounded border border-outline-variant text-on-surface-variant">
<span class="material-symbols-outlined text-[16px]">sync</span>
<span class="text-[12px] uppercase tracking-wider font-bold">Two-way sync</span>
</div>
<h1 class="text-[48px] md:text-[64px] leading-tight text-primary font-extrabold tracking-tight">
Turn your Notion database into an online calendar
</h1>
<p class="text-body-lg text-on-surface-variant max-w-[540px]">
Sync your Notion workspace with Apple Calendar, Google Calendar, or any CalDAV app. Edit freely from either side.
</p>
<div class="pt-sm">
<a class="bg-primary text-on-primary h-[48px] px-8 rounded-lg text-h3 inline-flex items-center gap-3 hover:opacity-90 transition-all active:scale-[0.98]" href="/me">
<span class="material-symbols-outlined" style="font-variation-settings: 'FILL' 1;">login</span>
Log in / Sign up
</a>
<p class="mt-4 text-label-md text-on-surface-variant flex items-center gap-2">
<span class="material-symbols-outlined text-[16px] text-secondary">verified</span>
Free for your first 6 months, unlimited. No credit card required.
</p>
</div>
</div>
<div class="relative group">
<div class="absolute -inset-4 bg-gradient-to-r from-secondary-container/10 to-primary/5 rounded-xl blur-2xl group-hover:opacity-75 transition-opacity"></div>
<div class="relative rounded-xl hairline-border overflow-hidden bg-white shadow-sm p-lg">
<div class="flex items-center justify-center h-64 bg-surface-container rounded-lg border border-dashed border-outline-variant">
<span class="material-symbols-outlined !text-[64px] text-outline opacity-40">calendar_month</span>
</div>
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="how-it-works">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">A simple process</h2>
<p class="text-body-md text-on-surface-variant mt-2">Start syncing your data in just 3 steps</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-3 gap-gutter">
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">link</span>
</div>
<h3 class="text-h3 text-primary mb-2">1. Connect Notion</h3>
<p class="text-body-md text-on-surface-variant">Securely connect your Notion workspace via OAuth.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">database</span>
</div>
<h3 class="text-h3 text-primary mb-2">2. Pick a database</h3>
<p class="text-body-md text-on-surface-variant">Choose the database you want to sync. Requires a Date property.</p>
</div>
<div class="bg-white p-lg hairline-border rounded-lg hover-lift">
<div class="w-10 h-10 bg-primary text-on-primary flex items-center justify-center rounded-md mb-md">
<span class="material-symbols-outlined">event_available</span>
</div>
<h3 class="text-h3 text-primary mb-2">3. Sync your calendar</h3>
<p class="text-body-md text-on-surface-variant">Subscribe from your calendar app (Apple, Google, Outlook) via the CalDAV link.</p>
</div>
</div>
</div>
</section>
<section class="max-w-[1280px] mx-auto px-margin-desktop py-xl">
<div class="bento-grid grid-rows-2">
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-white flex flex-col justify-between">
<div>
<h4 class="text-h2 text-primary mb-2">Real-time two-way sync</h4>
<p class="text-body-md text-on-surface-variant">Changes in Notion update your calendar instantly, and vice versa. Never miss a deadline.</p>
</div>
<div class="mt-xl h-32 bg-surface-container rounded-lg border border-dashed border-outline flex items-center justify-center">
<span class="material-symbols-outlined text-[48px] text-outline opacity-40">sync_alt</span>
</div>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-surface-container flex flex-col justify-center items-center text-center">
<span class="material-symbols-outlined text-[48px] mb-4 text-secondary">security</span>
<h4 class="text-h3 text-primary">Your data, your control</h4>
<p class="text-body-md text-on-surface-variant mt-2">Only reads/writes the pages you authorize. Disconnect anytime.</p>
</div>
<div class="col-span-12 md:col-span-4 p-lg hairline-border rounded-xl bg-white">
<h4 class="text-h3 text-primary mb-2">Simple pricing</h4>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Free for your first 6 months, unlimited
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Just $1/year after that
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> No credit card required to start
</li>
</ul>
</div>
<div class="col-span-12 md:col-span-8 p-lg hairline-border rounded-xl bg-primary text-on-primary flex items-center justify-between">
<div>
<h4 class="text-h2 mb-1">Works with any calendar app</h4>
<p class="text-label-md opacity-80 uppercase tracking-widest">Standard CalDAV protocol</p>
</div>
<div class="text-code bg-white/10 p-3 rounded hairline-border border-white/20">
notion-caldav.opendiy.vn/cal/...
</div>
</div>
</div>
</section>
<section class="bg-surface-container-low py-xl border-y border-outline-variant" id="pricing">
<div class="max-w-[1280px] mx-auto px-margin-desktop">
<div class="text-center mb-xl">
<h2 class="text-h1 text-primary">Pricing</h2>
<p class="text-body-md text-on-surface-variant mt-2">Free for 6 months, then just $1/year</p>
</div>
<div class="grid grid-cols-1 md:grid-cols-2 gap-gutter max-w-[720px] mx-auto">
<div class="bg-white p-lg hairline-border rounded-xl">
<h3 class="text-h2 text-primary mb-1">First 6 months</h3>
<p class="text-display text-primary mb-2">Free</p>
<p class="text-body-md text-on-surface-variant mb-md">For every new account, starting the day you sign up.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Unlimited events
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> Connect multiple databases
</li>
<li class="flex items-center gap-2 text-body-md text-on-surface-variant">
<span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span> No credit card required
</li>
</ul>
</div>
<div class="bg-primary text-on-primary p-lg hairline-border rounded-xl relative">
<div class="absolute -top-3 right-lg bg-secondary text-on-primary text-[11px] uppercase tracking-wider font-bold px-3 py-1 rounded-full">After 6 months</div>
<h3 class="text-h2 mb-1">Upgrade</h3>
<p class="text-display mb-2">$1<span class="text-h3 opacity-70">/year</span></p>
<p class="text-body-md opacity-80 mb-md">Subscribe early and you'll still get your full 6 free months first — billing only starts once your trial ends.</p>
<ul class="space-y-2">
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Unlimited events
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Secure checkout via Stripe
</li>
<li class="flex items-center gap-2 text-body-md opacity-90">
<span class="material-symbols-outlined text-[18px]">check_circle</span> Cancel anytime
</li>
</ul>
</div>
</div>
<p class="text-center text-label-md text-on-surface-variant mt-lg">Past your free 6 months without subscribing? You can still use NotionCal, capped at 10 new events/day.</p>
</div>
</section>
</main>
<footer class="border-t border-outline-variant bg-surface mt-xl">
<div class="max-w-[1280px] mx-auto w-full px-margin-desktop py-lg flex flex-col md:flex-row justify-between items-center gap-lg">
<div class="flex flex-col md:flex-row items-center gap-md">
<span class="text-h3 font-bold text-primary">NotionCal</span>
</div>
<div class="flex items-center gap-6">
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/privacy">Privacy Policy</a>
<a class="text-label-md text-on-surface-variant hover:text-primary transition-colors opacity-80 hover:opacity-100" href="/terms">Terms of Service</a>
</div>
</div>
</footer>
"##;

pub fn vi_data() -> LandingPageData {
    LandingPageData {
        html_lang: "vi".to_string(),
        page_title: "NotionCal - Đồng bộ hóa Notion với Calendar".to_string(),
        meta_description: "Đồng bộ hóa cơ sở dữ liệu Notion của bạn với Apple Calendar, Google Calendar hoặc bất kỳ ứng dụng CalDAV nào. Chỉnh sửa linh hoạt từ cả hai phía, 6 tháng đầu miễn phí.".to_string(),
        twitter_description: "Đồng bộ hóa cơ sở dữ liệu Notion của bạn với Apple Calendar, Google Calendar hoặc bất kỳ ứng dụng CalDAV nào.".to_string(),
        og_locale: "vi_VN".to_string(),
        lang_toggle_href: "/lang/en?next=/".to_string(),
        lang_toggle_label: "EN".to_string(),
        body_html: BODY_HTML_VI.to_string(),
    }
}

pub fn en_data() -> LandingPageData {
    LandingPageData {
        html_lang: "en".to_string(),
        page_title: "NotionCal - Sync Notion with your Calendar".to_string(),
        meta_description: "Sync your Notion database with Apple Calendar, Google Calendar, or any CalDAV app. Two-way editing, free for the first 6 months.".to_string(),
        twitter_description: "Sync your Notion database with Apple Calendar, Google Calendar, or any CalDAV app.".to_string(),
        og_locale: "en_US".to_string(),
        lang_toggle_href: "/lang/vi?next=/".to_string(),
        lang_toggle_label: "VI".to_string(),
        body_html: BODY_HTML_EN.to_string(),
    }
}

#[component]
pub fn LandingRoutePage() -> impl IntoView {
    let data = if crate::page_shell::detect_lang() == "en" {
        en_data()
    } else {
        vi_data()
    };
    view! { <LandingHead data=data.clone()/> <LandingPage data=data/> }
}

#[component]
fn LandingHead(data: LandingPageData) -> impl IntoView {
    view! {
        <leptos_meta::Html attr:class="light" attr:lang=data.html_lang.clone()/>
        <leptos_meta::Title text=data.page_title.clone()/>
        <leptos_meta::Meta name="viewport" content="width=device-width, initial-scale=1.0"/>
        <leptos_meta::Meta name="msvalidate.01" content=BING_VALIDATE/>
        <leptos_meta::Meta name="description" content=data.meta_description.clone()/>
        <leptos_meta::Link rel="canonical" href=CANONICAL_URL/>
        <leptos_meta::Link rel="icon" href="/favicon.svg" type_="image/svg+xml"/>
        <leptos_meta::Meta property="og:site_name" content="NotionCal"/>
        <leptos_meta::Meta property="og:type" content="website"/>
        <leptos_meta::Meta property="og:title" content=data.page_title.clone()/>
        <leptos_meta::Meta property="og:description" content=data.meta_description.clone()/>
        <leptos_meta::Meta property="og:url" content=CANONICAL_URL/>
        <leptos_meta::Meta property="og:locale" content=data.og_locale.clone()/>
        <leptos_meta::Meta name="twitter:card" content="summary"/>
        <leptos_meta::Meta name="twitter:title" content=data.page_title.clone()/>
        <leptos_meta::Meta name="twitter:description" content=data.twitter_description.clone()/>
        <leptos_meta::Link rel="stylesheet" href="/assets/style-auth-b.css"/>
        <leptos_meta::Link href=GOOGLE_FONTS_HREF rel="stylesheet"/>
        <leptos_meta::Style>{LANDING_HEAD_STYLE}</leptos_meta::Style>
    }
}

#[component]
pub fn LandingPage(data: LandingPageData) -> impl IntoView {
    view! {
        <div id="landing-root" class="bg-background text-on-surface" inner_html=data.body_html></div>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    #[test]
    fn renders_vi_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = view! { <LandingPage data=vi_data()/> }.to_html();
        assert!(html.contains("Đăng nhập"));
        assert!(html.contains("Chính sách bảo mật"));
    }

    #[test]
    fn renders_en_without_panicking() {
        any_spawner::Executor::init_futures_executor().ok();
        let html = view! { <LandingPage data=en_data()/> }.to_html();
        assert!(html.contains("Log in"));
        assert!(html.contains("Privacy Policy"));
    }
}
