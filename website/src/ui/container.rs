use topcoat::{
    Result,
    view::{Attributes, Child, StaticClass, View, class, component, view},
};

/// The page column: centred, with a gutter that grows with the screen.
const COLUMN: StaticClass = class!("mx-auto w-full max-w-[68rem] px-[clamp(1rem,4vw,2.5rem)]");

/// A `<div>` as wide as the page column. `attrs` go on the `<div>`, with
/// their classes added to its own.
#[component]
pub async fn container(
    #[default] mut attrs: Attributes,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! { <div class=(class!(COLUMN, attrs.remove("class"))) (attrs)>(child)</div> })
}
