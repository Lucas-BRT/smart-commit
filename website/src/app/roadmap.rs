use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::ui::{doc_link, docs, page_head};

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
                <li><strong>"Publish v0.2, then train v0.3."</strong> " The v0.2 fine-tunes are trained, but their repositories are private."</li>
                <li><strong>"A model that stays loaded."</strong> " Today most of a commit is loading the model. Started, off by default."</li>
                <li><strong>"A fine-tune as the default model."</strong></li>
                <li><strong>"Fine-tunes on the NPU and Intel GPU."</strong> " They run one model today."</li>
            </ol>
            <p>"The full roadmap, with the reasoning behind each item, is in the " doc_link(page: "roadmap.md", "repository") "."</p>
        </section>
        )
    })
}
