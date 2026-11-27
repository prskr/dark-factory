# Spec Delta

## ADDED Requirements

### Requirement: Table layout
A rendered Markdown table SHALL break a cell's text only between words and SHALL separate its rows with a line and its header row with a bold font, in agent bubbles, user bubbles, request cards and on panels alike. A table wider than its place SHALL scroll in its own box. Inline code outside an agent bubble SHALL use the background of its own surface, not the agent bubble's code color.

#### Scenario: Narrow column
- **WHEN** an agent message holds a table whose second column has a long sentence and the bubble is 390 px wide
- **THEN** every word of the first column stays on one line and the sentence wraps between words

#### Scenario: Rows are separated
- **WHEN** a `.md` artifact with a table of three rows is opened in Preview
- **THEN** a line separates each row from the next and the header row is bold

#### Scenario: Wide table
- **WHEN** a table holds a long URL in a cell
- **THEN** the table scrolls horizontally inside its box and the page does not scroll

#### Scenario: Inline code on a panel
- **WHEN** a Markdown preview with inline code is shown in the light theme
- **THEN** the inline code reaches at least 4.5:1 against its background
