use topcoat::{
    Result,
    router::href,
    view::{View, attributes, component, view},
};

use crate::{
    app::tutorial,
    components::button::{ButtonSize, ButtonVariant, button_variants},
    facts::{INSTALL_CMD, REPO},
    ui::{command_line::command_line, container::container},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        <section>
            container(
                attrs: attributes! { class="pt-12 text-center md:pt-20" },
                <h1
                    class="mx-auto max-w-[18ch] text-[clamp(2rem,4.8vw,3.5rem)] leading-[1.06]"
                >
                    "Commit messages in "
                    <em class="text-primary not-italic">"your repo's style"</em>
                </h1>
                <p class="mx-auto mt-5 max-w-[32rem] text-base text-muted-foreground">
                    "One command writes the commit message from your staged diff and your history."
                </p>
                <div class="mt-9 flex flex-wrap justify-center gap-3">
                    <a
                        href=(href!(tutorial::page))
                        class=(button_variants(
                            ButtonVariant::Primary,
                            ButtonSize::Lg,
                        ))
                    >
                        "Get started"
                    </a>
                    <a
                        href=(REPO)
                        class=(button_variants(
                            ButtonVariant::Outline,
                            ButtonSize::Lg,
                        ))
                    >
                        "GitHub"
                    </a>
                </div>
                command_line(
                    attrs: attributes! { class="mx-auto mt-6 w-full max-w-[52rem]" },
                    (INSTALL_CMD)
                )
            )
        </section>
    })
}
