# Spec Delta

## ADDED Requirements

### Requirement: Contrast check of the main pages
The suite SHALL check the text contrast of the main pages (request inbox, task timeline and details, chat, artifact view, Markdown preview, settings pages) in the light and the dark theme. It SHALL fail when text is below 4.5:1, or below 3:1 for large text, against its background, and SHALL name the page, the theme, the element and the measured ratio.

#### Scenario: Low-contrast text in dark mode
- **WHEN** a page of the check renders text below 4.5:1 only in the dark theme
- **THEN** the check fails and names the page, "dark", the element and the ratio

#### Scenario: Every main page in both themes
- **WHEN** the suite runs against a build without contrast defects
- **THEN** the check passes after visiting each main page once in the light and once in the dark theme

#### Scenario: Element the check cannot judge
- **WHEN** text sits on an image or a gradient, so its contrast cannot be measured
- **THEN** the check does not fail on it and lists it in the test's diagnostics
