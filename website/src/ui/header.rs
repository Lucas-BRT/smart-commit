use topcoat::{
    Result,
    context::Cx,
    router::href,
    view::{StaticClass, View, attributes, class, component, view},
};

use super::container::container;
use crate::{
    app::{install, tutorial},
    components::button::{ButtonSize, ButtonVariant, button_variants},
    facts::REPO,
    site,
};

/// A link of the main menu that is marked when its section is the page.
const CURRENT: StaticClass = class!("aria-[current=page]:bg-card aria-[current=page]:text-primary");

#[component]
pub async fn site_header(cx: &Cx) -> Result<impl View> {
    let in_docs = site::current(cx).is_some_and(|page| page.section.is_some());
    let menu = || {
        class!(
            button_variants(ButtonVariant::Ghost, ButtonSize::Sm),
            CURRENT
        )
    };

    Ok(view! {
        <header class="border-b border-border print:hidden">
            container(
                attrs: attributes! { class="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 py-4" },
                <a class="flex items-center gap-2.5 font-display text-xl font-bold tracking-tight text-foreground no-underline" href=(href!(crate::app::page))>
                    <img src="/logo.svg" alt="" class="size-8">
                    "smart-commit"
                </a>
                <nav aria-label="Main">
                    <ul class="m-0 flex list-none items-center gap-1 p-0">
                        <li class="m-0"><a class=(menu()) href=(href!(tutorial::page)) aria-current=(in_docs.then_some("page"))>"Docs"</a></li>
                        <li class="m-0"><a class=(menu()) href=(REPO)>"GitHub"</a></li>
                        <li class="m-0 ml-2"><a class=(button_variants(ButtonVariant::Primary, ButtonSize::Sm)) href=(href!(install::page))>"Install"</a></li>
                    </ul>
                </nav>
            )
        </header>
    })
}
