# markdown-rendering Specification

## Purpose

Defines how agent-written Markdown is shown to humans in the web UI: what is rendered, what is refused because the text is untrusted, how code blocks are highlighted and themed, and how Mermaid diagrams are drawn.

## Requirements

### Requirement: Untrusted Markdown
Rendered Markdown SHALL NOT pass through raw HTML: markup in the source SHALL be shown as text. Links with unsafe schemes SHALL be dropped, every link SHALL carry `rel="noopener noreferrer nofollow"`, and images SHALL be rendered as links, so the browser fetches nothing an agent names.

#### Scenario: Script in Markdown
- **WHEN** an agent message contains `<script>alert(1)</script>`
- **THEN** the text is shown literally and no script runs

#### Scenario: Image in Markdown
- **WHEN** an agent message contains `![logo](https://example.com/logo.png)`
- **THEN** a link to the URL is shown and the browser makes no request to `example.com`

#### Scenario: JavaScript link
- **WHEN** an agent message contains `[click](javascript:alert(1))`
- **THEN** no link with a `javascript:` URL is rendered

### Requirement: Code highlighting by fence language
A fenced code block SHALL be syntax-highlighted when its info string names a supported language by name or file extension. A block without a language, with an unsupported language or with more than 64 KiB of content SHALL be rendered as plain, escaped text. The language SHALL NOT be guessed from the content.

#### Scenario: Rust block
- **WHEN** a completed agent message contains a fence tagged `rust`
- **THEN** keywords, strings and comments of the block are highlighted

#### Scenario: Extension as language
- **WHEN** a fence is tagged `rs`
- **THEN** it is highlighted like a fence tagged `rust`

#### Scenario: No language
- **WHEN** a fence has no info string
- **THEN** its content is shown as plain text, without highlighting

#### Scenario: Unknown language
- **WHEN** a fence is tagged `no-such-language`
- **THEN** its content is shown as plain text and the message renders without error

#### Scenario: Oversized block
- **WHEN** a fence tagged `rust` holds more than 64 KiB
- **THEN** its content is shown as plain text

#### Scenario: Markup inside code
- **WHEN** a fence tagged `html` contains `<script>alert(1)</script>`
- **THEN** the code is shown highlighted as text and no script runs

### Requirement: Highlighting only completed Markdown
Markdown SHALL be rendered and highlighted only once its text is complete. An agent message SHALL stay plain text while it streams and be rendered when its turn ends.

#### Scenario: Streaming fence
- **WHEN** an agent streams a message containing a fence tagged `rust`
- **THEN** the message grows as plain text and the fence is highlighted only after the turn ends

### Requirement: Code themes per color scheme
Highlighted code SHALL use the theme configured for the light color scheme in the light UI theme and the theme configured for the dark color scheme in the dark UI theme, switching with `prefers-color-scheme` without a reload. A code block SHALL show its theme's background, inside agent message bubbles too.

#### Scenario: Default themes
- **WHEN** no code themes are configured
- **THEN** code uses `InspiredGitHub` in the light UI theme and `base16-ocean.dark` in the dark UI theme

#### Scenario: Scheme changes while the page is open
- **WHEN** the browser switches from light to dark while a task page with highlighted code is open
- **THEN** the code switches to the dark code theme without a reload

### Requirement: Code theme configuration
The instance config options `web.codeTheme.light` and `web.codeTheme.dark` SHALL each name one of the bundled themes: `InspiredGitHub`, `Solarized (light)`, `Solarized (dark)`, `base16-ocean.light`, `base16-ocean.dark`, `base16-eighties.dark`, `base16-mocha.dark`. An unknown name SHALL fail control plane startup with an error naming the option and the valid themes. Themes are read at startup.

#### Scenario: Operator picks Solarized
- **WHEN** the instance config sets `web.codeTheme.light: "Solarized (light)"` and `web.codeTheme.dark: "Solarized (dark)"`
- **THEN** code is shown in the Solarized themes after the control plane starts

#### Scenario: Typo in a theme name
- **WHEN** the instance config sets `web.codeTheme.dark: "Monokai"`
- **THEN** the control plane does not start and reports `web.codeTheme.dark` and the list of valid themes

### Requirement: Mermaid diagrams
A fenced code block tagged `mermaid` in completed Markdown SHALL be rendered as a diagram in the browser. Each diagram SHALL be isolated from the page so that diagram source cannot run script in the page or read its content. The diagram renderer SHALL be served by the control plane and loaded only on pages that contain a diagram.

#### Scenario: Flowchart
- **WHEN** a completed agent message contains a `mermaid` fence with `flowchart LR; A-->B`
- **THEN** a diagram with the nodes A and B is shown instead of the source

#### Scenario: Script in a diagram
- **WHEN** a `mermaid` fence contains a node label with `<img src=x onerror=alert(1)>`
- **THEN** no script runs in the page

#### Scenario: Diagram tries to lower its isolation
- **WHEN** a `mermaid` fence starts with `%%{init: {"securityLevel": "loose"}}%%` and contains a node label with `<img src=x onerror=alert(1)>`
- **THEN** the diagram stays isolated and no script runs in the page

#### Scenario: Page without diagrams
- **WHEN** a task page contains no `mermaid` fence
- **THEN** the page does not load the diagram renderer

### Requirement: Mermaid errors and theme
A diagram whose source cannot be parsed SHALL be shown as its source in a plain code block with a one-line error. Diagrams SHALL use a light or a dark diagram theme following `prefers-color-scheme` and SHALL be redrawn when it changes.

#### Scenario: Invalid diagram
- **WHEN** a `mermaid` fence contains `flowchart LR; A-->`
- **THEN** the source is shown as plain text with a one-line error, and the rest of the message renders normally

#### Scenario: Scheme changes
- **WHEN** the browser switches from light to dark while a diagram is shown
- **THEN** the diagram is redrawn in the dark diagram theme without a reload

### Requirement: Diagrams without JavaScript
Without JavaScript, a `mermaid` fence SHALL be shown as its source in a plain code block; all other Markdown, including highlighted code, SHALL render without JavaScript.

#### Scenario: Script disabled
- **WHEN** a page with a `mermaid` fence and a `rust` fence is opened with JavaScript disabled
- **THEN** the diagram source is shown as plain text and the Rust code is highlighted

### Requirement: Diagram renderer missing
When the control plane was built without Mermaid, or the renderer fails to load, a `mermaid` fence SHALL be shown as its source in a plain code block with the one-line error "Diagram renderer not available", and the rest of the page SHALL render normally.

#### Scenario: Build without Mermaid
- **WHEN** a control plane built by plain `cargo` without Mermaid shows a message with a `mermaid` fence
- **THEN** the page does not request `/assets/vendor/mermaid.min.js`, shows the diagram source, and shows "Diagram renderer not available" below it

#### Scenario: Renderer fails to load
- **WHEN** the request for `/assets/vendor/mermaid.min.js` fails
- **THEN** each diagram on the page shows its source and "Diagram renderer not available"

### Requirement: Markdown in user messages
The visible text of a user message in the transcript SHALL be rendered as Markdown with the same rules as agent messages (untrusted input, code highlighting by fence language, code themes, Mermaid), except that a single newline SHALL be a line break. One or more blank lines SHALL separate paragraphs, as in agent messages. Hidden blocks shown with `web.showHiddenBlocks` and the queued message stack SHALL stay plain text.

#### Scenario: Code in a user message
- **WHEN** a board member sends a message with a fence tagged `rust`
- **THEN** the message bubble shows the block highlighted like the same block in an agent message

#### Scenario: Line breaks kept
- **WHEN** a board member sends "first line" and "second line" separated by a single newline
- **THEN** the message shows them on two lines

#### Scenario: Several blank lines
- **WHEN** a board member sends "first" and "second" separated by three blank lines
- **THEN** the message shows two paragraphs with the usual paragraph spacing

#### Scenario: Script in a user message
- **WHEN** a user message contains `<script>alert(1)</script>`
- **THEN** the text is shown literally and no script runs

#### Scenario: Queued message stays plain
- **WHEN** a message with a fence tagged `rust` is queued
- **THEN** the queued stack shows its text plain, without highlighting
