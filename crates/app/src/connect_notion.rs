use leptos::prelude::*;

use crate::page_shell::ONBOARDING_HEAD_STYLE;

#[component]
pub fn ConnectNotionRoutePage() -> impl IntoView {
    #[cfg(feature = "ssr")]
    let email = crate::page_shell::current_user_email();
    #[cfg(not(feature = "ssr"))]
    let email = String::new();
    view! {
        <leptos_meta::Html attr:lang="en"/>
        <leptos_meta::Title text="Connect Notion — NotionCal"/>
        <leptos_meta::Style>{ONBOARDING_HEAD_STYLE}</leptos_meta::Style>
        <ConnectNotionPage email=email/>
    }
}

#[component]
pub fn ConnectNotionPage(email: String) -> impl IntoView {
    view! {
        <div id="connect-notion-root" class="pt-[64px]">
            <crate::page_shell::HomeHeader email=email/>
            <main class="flex-grow flex items-center justify-center px-margin-mobile md:px-margin-desktop">
                <div class="w-full max-w-[480px] bg-surface-container-lowest border border-outline-variant p-xl rounded-lg card-shadow">
                    <div class="flex justify-center items-center gap-md mb-lg">
                        <div class="w-12 h-12 flex items-center justify-center bg-surface-container border border-outline-variant rounded-xl">
                            <span class="material-symbols-outlined !text-[28px]">"link"</span>
                        </div>
                        <div class="w-2 h-[1px] bg-outline-variant"></div>
                        <div class="w-12 h-12 flex items-center justify-center bg-primary rounded-xl">
                            <svg class="w-7 h-7 text-white fill-current" viewBox="0 0 24 24"><path d="M4.459 4.208c.673-.51 1.258-.69 2.067-.69h11.974c.421 0 .762.341.762.762v15.44c0 .421-.341.762-.762.762H5.539c-.588 0-1.026-.411-1.127-1.002L3.13 10.985C3.01 10.378 3.167 9.754 3.56 9.27L4.459 4.208zM17.15 17.61V6.63h-.04l-3.32 4.45h-.04V6.63h-1.28v10.98h.04l3.32-4.44h.04v4.44h1.28z"></path></svg>
                        </div>
                    </div>
                    <div class="text-center mb-xl">
                        <h1 class="text-h1 mb-sm tracking-tight text-primary">"Connect your Notion workspace"</h1>
                        <p class="text-body-md text-on-surface-variant leading-relaxed">"We need access to your Notion workspace to find and sync the databases you choose. You'll pick exactly which pages to share in the next step, on Notion."</p>
                    </div>
                    <a class="w-full h-12 bg-primary text-white text-label-md flex items-center justify-center gap-sm rounded-lg hover:bg-zinc-800 transition-all active:scale-[0.98] mb-lg" href="/connect/notion/start">
                        <svg class="w-5 h-5 fill-current" viewBox="0 0 24 24"><path d="M4.459 4.208c.673-.51 1.258-.69 2.067-.69h11.974c.421 0 .762.341.762.762v15.44c0 .421-.341.762-.762.762H5.539c-.588 0-1.026-.411-1.127-1.002L3.13 10.985C3.01 10.378 3.167 9.754 3.56 9.27L4.459 4.208zM17.15 17.61V6.63h-.04l-3.32 4.45h-.04V6.63h-1.28v10.98h.04l3.32-4.44h.04v4.44h1.28z"></path></svg>
                        "Connect to Notion"
                    </a>
                    <div class="border-t border-outline-variant pt-lg space-y-md">
                        <div class="flex items-start gap-md">
                            <span class="material-symbols-outlined !text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">"check_circle"</span>
                            <span class="text-body-md text-on-surface-variant">"Only reads and writes the pages you allow"</span>
                        </div>
                        <div class="flex items-start gap-md">
                            <span class="material-symbols-outlined !text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">"check_circle"</span>
                            <span class="text-body-md text-on-surface-variant">"Disconnect anytime"</span>
                        </div>
                        <div class="flex items-start gap-md">
                            <span class="material-symbols-outlined !text-[20px] text-secondary mt-0.5" style="font-variation-settings: 'FILL' 1;">"check_circle"</span>
                            <span class="text-body-md text-on-surface-variant">"Never shares your data with third parties"</span>
                        </div>
                    </div>
                </div>
            </main>
            <div class="max-w-[1280px] mx-auto w-full px-margin-desktop">
                <crate::page_shell::PageFooter/>
            </div>
        </div>
    }
}
