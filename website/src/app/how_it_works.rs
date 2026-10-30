use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::ui::{doc_link, docs, page_head};

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(current: "/how-it-works",
        page_head(
            title: "How it works",
            lede: "The path from your history and staged diff to one message.",
        )

        <section class="rail" aria-labelledby="pipeline">
            <h2 id="pipeline">"The pipeline"</h2>
            <ol class="pipeline">
                <li><strong>"Read git"</strong><span>"The log, the branch name and " <code>"git diff --staged"</code> ", through the " <code>"git"</code> " binary so your git config applies."</span></li>
                <li><strong>"Infer the style"</strong><span>"Convention, types, scopes, ticket format and examples, from the commit subjects. Messages are always in English."</span></li>
                <li><strong>"Compact the diff"</strong><span>"Files are classified and fitted into a token budget, by priority."</span></li>
                <li><strong>"Build the prompt"</strong><span>"A system and a user message, within the device's prompt limit."</span></li>
                <li><strong>"Ask the model"</strong><span>"On the chosen device, with greedy decoding and explicit parameters."</span></li>
                <li><strong>"Sanitize"</strong><span>"Fences, quotes, preambles and long subjects are cleaned up; bad replies are rejected."</span></li>
            </ol>
        </section>

        <section class="rail" aria-labelledby="safe">
            <h2 id="safe">"Failures and fallbacks"</h2>
            <p>"A bad reply gets one retry, then a message built from the file list. A merge, a revert or a cherry-pick keeps the message git prepared, and a commit of lockfiles only is written from the file list. Neither loads the model. An error in the hook becomes a warning, and the commit goes on."</p>
            <p>"The full description is in the " doc_link(page: "architecture.md", "architecture guide") "."</p>
        </section>
        )
    })
}
