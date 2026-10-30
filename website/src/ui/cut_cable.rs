use topcoat::{
    Result,
    view::{Attributes, View, class, component, view},
};

/// An Ethernet cable cut in two: no connection. Drawn after an RJ45 plug,
/// which has a latch on top, eight contacts on its face, a strain relief
/// and the cable; the wires show where it was cut.
#[component]
pub async fn cut_cable(#[default] mut attrs: Attributes) -> Result<impl View> {
    Ok(view! {
        <svg
            class=(class!("w-full text-muted-foreground", attrs.remove("class")))
            viewBox="0 12 400 104"
            role="img"
            aria-label="An ethernet cable cut in two"
            (attrs)
        >
            <defs>
                <g id="rj45-half" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                    <path class="fill-background" d="M30 44L37 30H67L74 44"/>
                    <path d="M47 30v-5h10v5"/>
                    <rect class="fill-background" x="16" y="44" width="64" height="52" rx="6"/>
                    <path d="M25 56v22M31 56v22M37 56v22M43 56v22M49 56v22M55 56v22M61 56v22M67 56v22" stroke-width="2"/>
                    <rect class="fill-background" x="80" y="52" width="24" height="36" rx="7"/>
                    <path class="fill-background" d="M104 60h46l-6 6 8 5-7 6 5 7h-46z"/>
                    <path class="stroke-primary" d="M150 63c7-3 12-1 16 2M152 70c8 0 12 3 15 7M147 77c6 3 10 6 12 11" stroke-width="2.5"/>
                </g>
            </defs>
            <use href="#rj45-half"></use>
            <use href="#rj45-half" transform="translate(400 0) scale(-1 1)"></use>
            <g class="stroke-primary" fill="none" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M200 38V20M184 48l-9-9M216 48l9-9"/>
                <path d="M182 108l9-6 9 6 9-6 9 6"/>
            </g>
        </svg>
    })
}
