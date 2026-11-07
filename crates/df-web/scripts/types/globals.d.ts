// Other globals the components use: htmx (vendored) and Mermaid (loaded on demand).

declare const htmx: {
  process(element: Element): void;
  registerExtension(name: string, hooks: Record<string, (element: Element) => void>): void;
};

/** The state the htmx SSE extension (`hx-sse`) keeps on a connect element. */
interface SseElement extends Element {
  _htmx?: { sse?: { abortController?: AbortController; reader?: ReadableStreamDefaultReader | null } };
}

interface MermaidApi {
  initialize(config: Record<string, unknown>): void;
  render(id: string, source: string): Promise<{ svg: string }>;
}

interface Window {
  mermaid: MermaidApi;
}
