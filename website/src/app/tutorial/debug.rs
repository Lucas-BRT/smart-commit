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
            id: "wrong",
            title: "6. When a message is wrong",
            note: "(optional)",
            terminal(
                title: "debug",
                label: "See what was decided, or record a generation",
                "smart-commit -v\n"
                note("also print what was decided and why")
                "smart-commit --trace bad.json\n"
                note("record the generation, to report it")
            )
            <p>
                "A trace holds your staged diff. Review it before sharing."
            </p>
        )
    })
}
