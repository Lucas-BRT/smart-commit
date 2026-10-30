use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::ui::{doc_link, docs, facts::NEXT, page_head};

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(current: "/roadmap",
        page_head(
            title: "Roadmap",
            lede: "Next items, highest priority first.",
        )

        <section class="rail" aria-labelledby="next">
            <h2 id="next">"Next"</h2>
            <ol>
                for item in NEXT {
                    <li>(*item)</li>
                }
            </ol>
            <p>"These are the first items of the roadmap. The whole list, with the reasoning behind each item, is in the " doc_link(page: "roadmap.md", "repository") "."</p>
        </section>
        )
    })
}
