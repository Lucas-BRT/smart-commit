use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::ui::{
    container::container,
    terminal::{cmd, message, note, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        <section class="mt-20 md:mt-24" aria-labelledby="history">
            container(
                attrs: attributes! {
                    class="grid items-center gap-10 lg:grid-cols-2 lg:gap-20"
                },
                <div>
                    <h2 id="history">"Reads your history"</h2>
                    <p class="mt-5 text-muted-foreground">
                        "The convention, scopes and ticket format come from your commit subjects."
                    </p>
                    <p class="text-muted-foreground">
                        "The "
                        <code>"prepare-commit-msg"</code>
                        " hook fills the editor on every plain "
                        <code>"git commit"</code>
                        "."
                    </p>
                </div>
                terminal(
                    title: "~/projects/api",
                    label: "Committing with the hook installed",
                    cmd("smart-commit hook install")
                    cmd("git add .")
                    cmd("git commit")
                    note("the editor opens with the generated message")
                    "\n"
                    message("refactor(auth): move session handling into its own module")
                )
            )
        </section>
    })
}
