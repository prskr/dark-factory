// Other globals the components use: htmx (vendored) and Mermaid (loaded on demand).

declare const htmx: {
  process(element: Element): void;
  /** Swaps `text` into `target` like a response: `swap` is an `hx-swap` style such as `innerMorph`. */
  swap(context: { target: Element; text: string; swap: string; sourceElement?: Element }): Promise<void>;
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

/** The highlighting editor of `/assets/editor.js` (`editor/markdown-editor.ts`), loaded on demand. */
interface MarkdownEditorApi {
  attach(textarea: HTMLTextAreaElement, options?: { describedBy?: string[] }): { destroy(): void; focus(): void };
}

interface Window {
  mermaid: MermaidApi;
  /** Defined once the bundle is loaded: `markdownField` checks that before it attaches. */
  DfMarkdownEditor: MarkdownEditorApi;
}
