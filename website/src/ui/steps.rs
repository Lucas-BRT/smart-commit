use topcoat::{
    Result,
    view::{Child, StaticClass, View, class, component, view},
};

const STEP: StaticClass = class!(
    "relative m-0 border-t border-border py-4 pl-12 [counter-increment:step]",
    "before:absolute before:top-4 before:left-0 before:grid before:size-8 before:place-items-center",
    "before:rounded-full before:border before:border-primary before:font-mono before:text-sm",
    "before:text-primary before:content-[counter(step)]",
);

/// A numbered list for a real sequence, such as the pipeline.
#[component]
pub async fn steps(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <ol class="m-0 max-w-none list-none p-0 [counter-reset:step]">(child)</ol>
    })
}

/// One step: its name, then what it does.
#[component]
pub async fn step(name: &str, #[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <li class=(STEP)>
            <strong class="block font-display text-[1.1rem] tracking-tight">(name)</strong>
            <span class="block text-[0.98rem] text-muted-foreground">(child)</span>
        </li>
    })
}
