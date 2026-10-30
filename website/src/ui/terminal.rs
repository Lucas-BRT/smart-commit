use topcoat::{
    Result,
    view::{Child, StaticClass, View, class, component, view},
};

const FRAME: StaticClass =
    class!("mx-0 my-6 overflow-hidden rounded-xl border border-terminal-border bg-terminal");
const TITLE: StaticClass = class!(
    "border-b border-terminal-border px-4 py-2.5 font-mono text-[0.78rem] text-terminal-dim"
);
const SCREEN: StaticClass = class!(
    "m-0 overflow-x-auto px-5 py-[1.1rem] font-mono text-[0.9rem] leading-[1.65]",
    "text-terminal-foreground max-sm:px-4 max-sm:py-3.5 max-sm:text-[0.8rem]",
);

/// A dark terminal window around preformatted text. Lines wrap, unless
/// `aligned`: output in columns keeps its shape and scrolls instead. The
/// screen can scroll sideways, so it takes the keyboard focus.
#[component]
pub async fn terminal(
    title: &str,
    label: &str,
    #[default] aligned: bool,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <figure class=(FRAME)>
            <div class=(TITLE) aria-hidden="true">(title)</div>
            <pre
                class=(class!(
                    SCREEN,
                    "whitespace-pre max-sm:text-[0.74rem]" if aligned else "whitespace-pre-wrap [overflow-wrap:anywhere]",
                ))
                tabindex="0"
                role="region"
                aria-label=(label)
            >(child)</pre>
        </figure>
    })
}

/// A command line: the prompt, the command and the end of the line.
#[component]
pub async fn cmd(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <span class="text-primary select-none">"$ "</span>(child)"\n"
    })
}

/// A note beside a command, dimmed, on a line of its own.
#[component]
pub async fn note(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { <span class="text-terminal-dim">"# "(child)</span>"\n" })
}

/// Output that is not the message: shown dimmed.
#[component]
pub async fn dim(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { <span class="text-terminal-dim">(child)</span> })
}

/// The commit message the tool wrote.
#[component]
pub async fn message(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { <span class="font-bold text-primary">(child)</span> })
}

/// The choice a table of results marks as selected.
#[component]
pub async fn selected(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { <span class="text-primary">(child)</span> })
}

/// A drawing in box characters, which keeps its lines.
#[component]
pub async fn boxed(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { <span class="block whitespace-pre max-sm:text-[0.7rem]">(child)</span> })
}
