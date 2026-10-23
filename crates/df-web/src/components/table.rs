//! The data table of the configuration lists (Projects, Credentials): one card, header row, cell
//! padding and row borders (`Projects.dc.html` / `Credentials.dc.html`).

use maud::{Markup, html};

/// Kind of a table cell: padding, font and alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// First column: the name of the entry.
    First,
    /// Plain text.
    Text,
    /// Monospace, for URLs, branches and keys.
    Mono,
    /// A right-aligned number or counter.
    Number,
    /// The edit button at the end of a row.
    Actions,
}

/// A header cell (`<th scope="col">`).
pub fn th(cell: Cell, content: Markup) -> Markup {
    let class = match cell {
        Cell::First => "px-5 py-3 font-medium",
        Cell::Text | Cell::Mono => "px-3 py-3 font-medium",
        Cell::Number => "px-3 py-3 text-right font-medium",
        Cell::Actions => "w-11 py-3 pr-5 pl-3",
    };
    html! { th scope="col" class=(class) { (content) } }
}

/// A body cell.
pub fn td(cell: Cell, content: Markup) -> Markup {
    let class = match cell {
        Cell::First => "px-5 py-3.5 font-medium",
        Cell::Text => "px-3 py-3.5",
        Cell::Mono => "px-3 py-3.5 font-mono text-[13px] text-muted",
        Cell::Number => "px-3 py-1 text-right",
        Cell::Actions => "py-2 pr-5 pl-3",
    };
    html! { td class=(class) { (content) } }
}

/// A body row.
pub fn tr(cells: Markup) -> Markup {
    html! { tr class="border-t border-line" { (cells) } }
}

/// The table card. `head` holds the [`th`] cells, `rows` the [`tr`] rows. A wide table scrolls
/// inside its card (`min-w-0`, `overflow-x-auto`; `relative`, so the card clips `sr-only` and other absolutely positioned descendants) and never widens the page.
pub fn data_table(head: Markup, rows: Markup) -> Markup {
    html! {
        div data-table class="relative min-w-0 max-w-full overflow-x-auto rounded-xl border border-line bg-panel" {
            table class="w-full border-collapse text-left text-sm" {
                thead { tr class="text-xs font-medium uppercase tracking-wide text-faint" { (head) } }
                tbody { (rows) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_scrolls_inside_itself_and_cells_follow_their_kind() {
        let html = data_table(
            html! { (th(Cell::First, html! { "Name" })) (th(Cell::Number, html! { "Done" })) (th(Cell::Actions, html! { span class="sr-only" { "Actions" } })) },
            html! { (tr(html! { (td(Cell::First, html! { "DF" })) (td(Cell::Number, html! { "3" })) (td(Cell::Actions, html! {})) })) },
        )
        .into_string();
        assert!(html.starts_with("<div data-table class=\"relative min-w-0 max-w-full overflow-x-auto rounded-xl border border-line bg-panel\">"), "{html}");
        assert!(
            html.contains("<th scope=\"col\" class=\"px-5 py-3 font-medium\">Name</th>"),
            "{html}"
        );
        assert!(
            html.contains("<th scope=\"col\" class=\"px-3 py-3 text-right font-medium\">Done</th>")
        );
        assert!(html.contains("<th scope=\"col\" class=\"w-11 py-3 pr-5 pl-3\">"));
        assert!(
            html.contains(
                "<tr class=\"border-t border-line\"><td class=\"px-5 py-3.5 font-medium\">DF</td>"
            ),
            "{html}"
        );
        assert!(html.contains("<td class=\"px-3 py-1 text-right\">3</td>"));
        assert!(html.contains("<td class=\"py-2 pr-5 pl-3\"></td>"));
        let mono = td(Cell::Mono, html! { "main" }).into_string();
        assert!(
            mono.contains("font-mono") && mono.contains("text-muted"),
            "{mono}"
        );
        let text = td(Cell::Text, html! { "x" }).into_string();
        assert!(!text.contains("font-mono"), "{text}");
    }
}
