use topcoat::{
    Result,
    router::href,
    view::{Child, View, component, view},
};

use crate::{app::how_it_works, ui::container::container};

/// One step of the pipeline, numbered.
#[component]
async fn step(number: u8, name: &str, #[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <li class="m-0 border-t border-border pt-4">
            <span class="font-mono text-sm text-primary">(number)</span>
            <strong class="mt-2 block font-display text-lg tracking-tight">
                (name)
            </strong>
            <span class="mt-2 block text-sm text-muted-foreground">(child)</span>
        </li>
    })
}

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        <section class="mt-24 pb-20 md:mt-32 md:pb-24" aria-labelledby="how">
            container(
                <div class="flex flex-wrap items-end justify-between gap-x-8 gap-y-2">
                    <h2 id="how">"How a message is made"</h2>
                    <a class="font-semibold" href=(href!(how_it_works::page))>
                        "The full pipeline"
                    </a>
                </div>
                <ol
                    class="m-0 mt-10 grid max-w-none list-none gap-x-6 gap-y-8 p-0 md:grid-cols-5"
                >
                    step(
                        number: 1,
                        name: "Read git",
                        "The log, the branch and the staged diff."
                    )
                    step(
                        number: 2,
                        name: "Infer the style",
                        "Convention, scopes and ticket format from the commit subjects."
                    )
                    step(
                        number: 3,
                        name: "Fit the diff",
                        "Files are ranked into the model's token budget."
                    )
                    step(
                        number: 4,
                        name: "Ask the model",
                        "Greedy decoding, on the fastest device."
                    )
                    step(
                        number: 5,
                        name: "Clean up",
                        "A bad reply is retried once, then falls back to the file list."
                    )
                </ol>
            )
        </section>
    })
}
