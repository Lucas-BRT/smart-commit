use topcoat::{
    Result,
    icon::IconData,
    router::href,
    view::{Child, View, component, view},
};

use crate::{
    app::{devices, measurements},
    components::card::{card, card_content, card_description, card_header, card_title},
    measures::{by_device, default_model_on},
    ui::{
        container::container,
        icons::{CPU, GPU, NPU, card_icon},
    },
};

/// A kind of device: what it is, and the times it was measured at.
#[component]
async fn device_card(
    icon: IconData,
    name: &str,
    summary: &str,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        card(
            card_header(
                card_icon(data: icon)
                card_title((name))
                card_description((summary))
            )
            card_content(
                <ul
                    class="m-0 grid max-w-none list-none gap-1.5 p-0 text-sm text-muted-foreground"
                >
                    (child)
                </ul>
            )
        )
    })
}

/// A time, then the machine it was measured on.
#[component]
async fn measured(time: &str, machine: &str) -> Result<impl View> {
    Ok(view! {
        <li class="m-0">
            <span class="font-mono text-primary">(time)</span>
            " on "
            (machine)
        </li>
    })
}

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        <section class="mt-24 md:mt-32" aria-labelledby="devices">
            container(
                <h2 id="devices">"Runs on the hardware you have"</h2>
                <p class="mt-4 max-w-[34rem] text-muted-foreground">
                    "Setup picks the fastest device in your machine. Fractions of seconds per message, with the model loaded:"
                </p>
                <div class="mt-8 grid gap-4 md:grid-cols-3">
                    device_card(
                        icon: CPU,
                        name: "CPU",
                        summary: "Any x86_64 processor, Intel or AMD.",
                        measured(
                            time: default_model_on("cpu").kept,
                            machine: "a Ryzen 5 5500"
                        )
                    )
                    device_card(
                        icon: GPU,
                        name: "GPU",
                        summary: "Integrated or Dedicated; Intel, AMD or NVIDIA.",
                        measured(
                            time: default_model_on("vulkan").kept,
                            machine: "an RTX 3050 with Ryzen 5 5500"
                        )
                    )
                    device_card(
                        icon: NPU,
                        name: "NPU",
                        summary: "The AI chip of Intel Core Ultra. Does not keep the model loaded yet.",
                        measured(time: by_device("npu"), machine: "a Core Ultra 5 225H")
                    )

                </div>
                <p class="mt-6 mb-0 text-sm text-muted-foreground">
                    "See details for: "
                    <a href=(href!(measurements::page))>"all the measurements"</a>
                    " and "
                    <a href=(href!(devices::page))>"how each device is supported"</a>
                    "."
                </p>
            )
        </section>
    })
}
