use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::{
    facts::{DEFAULT_MODEL_SIZE, LLAMACPP_SIZE, MIN_GLIBC, PLATFORM},
    ui::docs::doc_section,
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "requirements",
            title: "Requirements",
            <ul>
                <li>
                    "Platform: "
                    (PLATFORM)
                    ", with glibc "
                    (MIN_GLIBC)
                    " or newer, and git."
                </li>
                <li>
                    "Downloaded by the setup: the default model, "
                    (DEFAULT_MODEL_SIZE)
                    ", and the pinned llama.cpp build, "
                    (LLAMACPP_SIZE)
                    "."
                </li>
                <li>
                    "Distributions: Ubuntu 22.04 and newer, Debian 12+, Fedora 36+. Not Alpine or other musl systems."
                </li>
                <li>
                    "Disk: about 1.5 GB free in "
                    <code>"~/.local/share"</code>
                    " for the default model."
                </li>
                <li>
                    "Models and llama.cpp go to "
                    <code>"~/.local/share/smart-commit"</code>
                    "; caches go to "
                    <code>"~/.cache/smart-commit"</code>
                    "."
                </li>
            </ul>
        )
    })
}
