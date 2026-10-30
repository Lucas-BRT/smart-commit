use topcoat::{
    Result,
    view::{Attributes, Child, StaticClass, View, class, component, view},
};

const BAR: StaticClass = class!(
    "flex items-center gap-3 rounded-xl border border-border bg-terminal px-4 py-3.5",
    "text-left font-mono text-[0.85rem] leading-relaxed text-terminal-foreground",
    "select-all [overflow-wrap:anywhere]",
);

/// A command on its own, in a bar that selects whole when clicked.
#[component]
pub async fn command_line(
    #[default] mut attrs: Attributes,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <div class=(class!(BAR, attrs.remove("class"))) (attrs)>
            <span class="text-primary">"$"</span>
            <span>(child)</span>
        </div>
    })
}
