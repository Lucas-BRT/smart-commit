use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::{
    measures::{RESIDENT, plain},
    ui::{
        data_table::{cell, data_table, row},
        terminal::{note, terminal},
    },
};

/// What keeping the model loaded costs, and when it is not used.
#[component]
pub async fn costs() -> Result<impl View> {
    Ok(view! {
        <h3 class="mt-10">"What it costs, and when it is not used"</h3>
        <ul class="mt-3">
            <li>
                "It is kept only if it takes at most 15% of the RAM (" <code>"model.keep_loaded_max_ram"</code> ")."
            </li>
            <li>
                "The NPU and the Intel GPU have no server. On the NPU the first load compiles the graph (11.7s); later loads read the cache (1.7s, 246 MB)."
            </li>
            <li>
                "It stops when idle, or when a "
                <code>"model.*"</code>
                " setting changes."
            </li>
        </ul>
        data_table(
            label: "Memory a server keeps on the CPU",
            head: &["Model", "File", "With fast load", "Without it"],
            for server in RESIDENT {
                row(
                    cell(label: "Model", <code>(plain(server.model))</code>)
                    cell(label: "File", (server.file))
                    cell(label: "With fast load", (server.with_fast_load))
                    cell(label: "Without it", (server.without_fast_load))
                )
            }
        )
        terminal(
            title: "keeping the model loaded",
            label: "Commands for the kept model",
            "smart-commit server status\n"
            note("what is kept loaded, and how long it has been idle")
            "smart-commit server stop\n"
            note("free its memory now")
            "smart-commit config set model.keep_loaded 30\n"
            note("idle minutes; 0 turns it off")
        )
    })
}
