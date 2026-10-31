use topcoat::{
    Result,
    icon::IconData,
    view::{Child, StaticClass, View, attributes, class, component, view},
};

use crate::{
    facts::DEFAULT_MODEL_SIZE,
    ui::{
        container::container,
        cut_cable::cut_cable,
        icons::{NO_ACCOUNT, OFFLINE, PACKAGE, icon_badge},
    },
};

const PANEL: StaticClass = class!(
    "grid items-center gap-10 rounded-3xl border border-border bg-card p-8",
    "md:p-14 lg:grid-cols-2 lg:gap-12",
);

/// One thing the tool does not need, with its icon.
#[component]
async fn point(icon: IconData, title: &str, #[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <li class="m-0 flex gap-4">
            icon_badge(data: icon)
            <div>
                <h3>(title)</h3>
                <p class="mt-1 mb-0 text-muted-foreground">(child)</p>
            </div>
        </li>
    })
}

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        <section class="mt-24 md:mt-32" aria-labelledby="private">
            container(
                <div class=(PANEL)>
                    <div>
                        <h2 id="private">"Nothing leaves your machine"</h2>
                        <ul class="m-0 mt-8 grid max-w-none list-none gap-6 p-0">
                            point(
                                icon: NO_ACCOUNT,
                                title: "Nothing to sign up for",
                                "No account, no API key, no subscription."
                            )
                            point(
                                icon: PACKAGE,
                                title: "A small model",
                                "A "
                                (DEFAULT_MODEL_SIZE)
                                " download that runs on your hardware."
                            )
                            point(
                                icon: OFFLINE,
                                title: "Offline to write",
                                "Download once, run anywhare."
                            )
                        </ul>
                    </div>
                    cut_cable(attrs: attributes! { class="mx-auto max-w-[32rem]" })
                </div>
            )
        </section>
    })
}
