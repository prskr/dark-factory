// The part of the Alpine.js API the components use. Extend it when a component needs more.

/** Magic properties Alpine makes available as `this` in a component's methods. */
interface AlpineMagics {
  $el: HTMLElement;
  $refs: Record<string, HTMLElement>;
  $watch(property: string, callback: (value: unknown) => void): void;
  $nextTick(callback?: () => void): Promise<void>;
}

declare const Alpine: {
  /** Registers a component; `factory` returns its state and methods. */
  data<Args extends unknown[], State extends object>(
    name: string,
    factory: (...args: Args) => State & ThisType<State & AlpineMagics>,
  ): void;
  /** The state of the closest component of an element. */
  $data(el: Element): any;
};
