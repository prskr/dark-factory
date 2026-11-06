// Other globals the components use: htmx (vendored) and Mermaid (loaded on demand).

declare const htmx: {
  process(element: Element): void;
};

interface MermaidApi {
  initialize(config: Record<string, unknown>): void;
  render(id: string, source: string): Promise<{ svg: string }>;
}

interface Window {
  mermaid: MermaidApi;
}
