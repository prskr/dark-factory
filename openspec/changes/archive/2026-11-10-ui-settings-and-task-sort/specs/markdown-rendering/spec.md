# Spec Delta

## ADDED Requirements

### Requirement: Diagram renderer missing
When the control plane was built without Mermaid, or the renderer fails to load, a `mermaid` fence SHALL be shown as its source in a plain code block with the one-line error "Diagram renderer not available", and the rest of the page SHALL render normally.

#### Scenario: Build without Mermaid
- **WHEN** a control plane built by plain `cargo` without Mermaid shows a message with a `mermaid` fence
- **THEN** the page does not request `/assets/vendor/mermaid.min.js`, shows the diagram source, and shows "Diagram renderer not available" below it

#### Scenario: Renderer fails to load
- **WHEN** the request for `/assets/vendor/mermaid.min.js` fails
- **THEN** each diagram on the page shows its source and "Diagram renderer not available"
