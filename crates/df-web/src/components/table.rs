//! The data table of the configuration lists: one card, header row, cell padding and row borders.
//! The two-line rows (`PenPot`: "Settings · projects, two-line rows (1350)", "Model providers ·
//! two-line rows (1350)", "Credentials · two-line rows (1350)") are built from [`entry_tr`],
//! [`name_cell`], [`entry_link`] or [`entry_button`] and [`actions_cell`]: the entry control
//! covers the whole row, so the row opens the entry.

use maud::{Markup, html};

/// The entry control that covers the row; its cover draws the focus ring.
const ENTRY_CLASS: &str = "block truncate text-left font-medium outline-none group-hover:underline after:absolute after:inset-0 focus-visible:after:ring-2 focus-visible:after:ring-inset focus-visible:after:ring-ink";

/// Kind of a table cell: padding, font and alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// First column: the name of the entry.
    First,
    /// Plain text.
    Text,
    /// Monospace, for URLs, branches and keys.
    Mono,
    /// Plain text that stays on one line (the name column takes the remaining width).
    NoWrap,
    /// A right-aligned number or counter.
    Number,
    /// The controls at the end of a row (no edit button in the two-line lists).
    Actions,
}

/// A header cell (`<th scope="col">`).
pub fn th(cell: Cell, content: Markup) -> Markup {
    let class = match cell {
        Cell::First => "px-5 py-3 font-medium",
        Cell::Text | Cell::Mono => "px-3 py-3 font-medium",
        Cell::NoWrap => "whitespace-nowrap px-3 py-3 font-medium",
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
        Cell::NoWrap => "whitespace-nowrap px-3 py-3.5",
        Cell::Number => "px-3 py-1 text-right",
        Cell::Actions => "py-2 pr-5 pl-3",
    };
    html! { td class=(class) { (content) } }
}

/// A body row.
pub fn tr(cells: Markup) -> Markup {
    tr_with_id(None, cells)
}

/// A body row with an element id, for rows that a live update replaces.
pub fn tr_with_id(id: Option<&str>, cells: Markup) -> Markup {
    html! { tr id=[id] class="border-t border-line" { (cells) } }
}

/// A two-line row that opens its entry: `relative group`, shaded on hover. The entry control of
/// its [`name_cell`] covers it.
pub fn entry_tr(cells: Markup) -> Markup {
    html! { tr class="group relative border-t border-line hover:bg-sunk" { (cells) } }
}

/// The cell of a two-line row that takes the remaining width: `entry` is the name line
/// ([`entry_link`] or [`entry_button`]), `details` the muted line below. `max-w-0` keeps a long
/// details line from widening the table; both lines truncate.
pub fn name_cell(entry: Markup, details: Markup) -> Markup {
    html! {
        td class="w-full max-w-0 py-3.5 pr-3 pl-5" {
            (entry)
            div class="mt-0.5 truncate text-[13px] text-muted" { (details) }
        }
    }
}

/// A decorative separator between the parts of a details line (hidden from assistive technology),
/// with space on both sides.
pub fn separator(glyph: &str) -> Markup {
    html! { " " span aria-hidden="true" class="mx-1" { (glyph) } " " }
}

/// A faint separator that is part of the text (`secret / key`): assistive technology reads it.
pub fn faint_separator(glyph: &str) -> Markup {
    html! { " " span class="mx-1 text-faint" { (glyph) } " " }
}

/// The entry control of a page: a link named after the entry.
pub fn entry_link(href: &str, name: &str) -> Markup {
    html! { a href=(href) class=(ENTRY_CLASS) { (name) } }
}

/// The entry control of a modal: a button that loads the form from `href` into the composer slot.
pub fn entry_button(href: &str, label: &str, name: &str) -> Markup {
    html! {
        button type="button" hx-get=(href) hx-target="#composer-slot" hx-swap="innerHTML" aria-label=(label) class={"w-full " (ENTRY_CLASS)} { (name) }
    }
}

/// The action cell of a two-line row: its content sits above the row's cover (`relative z-10`).
pub fn actions_cell(content: Markup) -> Markup {
    td(
        Cell::Actions,
        html! { div class="relative z-10 flex items-center justify-end gap-1" { (content) } },
    )
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

    #[test]
    fn a_two_line_row_is_one_cover_over_a_truncating_name_and_details() {
        let cell = name_cell(
            entry_link("/projects/DF", "Dark Factory"),
            html! { "details" },
        )
        .into_string();
        assert!(cell.contains("<td class=\"w-full max-w-0 "), "{cell}");
        assert!(
            cell.contains("truncate text-[13px] text-muted\">details</div>"),
            "{cell}"
        );
        let row = entry_tr(html! {}).into_string();
        assert!(
            row.contains("group relative") && row.contains("hover:bg-sunk"),
            "{row}"
        );
        let link = entry_link("/projects/DF", "Dark Factory").into_string();
        for class in [
            "after:absolute",
            "after:inset-0",
            "group-hover:underline",
            "outline-none",
            "focus-visible:after:ring-2",
            "focus-visible:after:ring-inset",
            "focus-visible:after:ring-ink",
            "truncate",
        ] {
            assert!(link.contains(class), "{class}: {link}");
        }
        assert!(link.starts_with("<a href=\"/projects/DF\""), "{link}");
    }

    #[test]
    fn only_the_entry_control_draws_the_row_focus_ring() {
        let row = entry_tr(html! {
            (name_cell(entry_link("/m/1", "OpenAI"), html! { "details" }))
            (actions_cell(html! { button type="button" aria-label="Delete" { "x" } }))
        })
        .into_string();
        assert_eq!(row.matches("focus-visible:after:ring").count(), 3, "{row}");
        let actions = &row[row.find("<td class=\"py-2").unwrap()..];
        assert!(!actions.contains("ring"), "{actions}");
        assert!(!actions.contains("after:"), "{actions}");
        let entry = &row[row.find("<a href").unwrap()..row.find("</a>").unwrap()];
        assert_eq!(
            entry.matches("focus-visible:after:ring").count(),
            3,
            "{entry}"
        );
    }

    #[test]
    fn the_modal_entry_is_a_labeled_button_and_actions_sit_above_the_cover() {
        let button =
            entry_button("/credentials/1/edit", "Edit credential reference a", "a").into_string();
        for part in [
            "<button type=\"button\"",
            "hx-get=\"/credentials/1/edit\"",
            "hx-target=\"#composer-slot\"",
            "aria-label=\"Edit credential reference a\"",
            "after:inset-0",
        ] {
            assert!(button.contains(part), "{part}: {button}");
        }
        let actions = actions_cell(html! { "x" }).into_string();
        assert!(actions.contains("<div class=\"relative z-10 "), "{actions}");
    }
}
