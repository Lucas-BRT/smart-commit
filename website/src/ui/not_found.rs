use topcoat::{
    Result,
    router::StatusCode,
    view::{View, attributes, component, view},
};

use super::container::container;

/// What a page that does not exist shows. It answers with 404.
#[component]
pub async fn not_found() -> Result<impl View> {
    Ok(view! {
        (StatusCode::NOT_FOUND)
        container(
            attrs: attributes! { class="pt-14 pb-24" },
            <h1>"Page not found"</h1>
            <p class="mt-4 text-muted-foreground">
                "This page does not exist. "
                <a href=(topcoat::router::href!(crate::app::page))>"Go to the home page"</a>
                "."
            </p>
        )
    })
}
