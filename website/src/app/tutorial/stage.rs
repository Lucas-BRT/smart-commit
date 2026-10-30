use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    docs::doc_section,
    terminal::{note, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "stage",
            title: "2. Stage your changes",
            terminal(
                title: "stage",
                label: "Staging changes",
                "git add src/retry.rs\n"
                note("or, hunk by hunk")
                "git add -p"
            )
            <p>
                <code>"-p"</code>
                " keeps each commit focused, but any staging works."
            </p>
            <p>
                "It reads only what is staged. In a merge, revert or cherry-pick it keeps git's own message."
            </p>
        )
    })
}
