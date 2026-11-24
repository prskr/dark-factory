// Alpine component of the persona form (`x-data='personaAvatar("<display name>")'`, see
// `config_pages/personas.rs`): the avatar preview follows the display name without a reload.
//
// State: `name` (the display name as typed), `initials` (getter): the first letters of up to two
// words, upper case, `?` for an empty name, as `components::shell::initials` renders them.
// `track(event)`: copies the value of the `display_name` input of an `input` event into `name`.
function initialsOf(name: string): string {
  const letters = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => Array.from(word)[0].toUpperCase())
    .join('');
  return letters || '?';
}

document.addEventListener('alpine:init', () => {
  Alpine.data('personaAvatar', (name: string) => ({
    name,

    get initials(): string {
      return initialsOf(this.name);
    },

    track(event: Event): void {
      const input = event.target as HTMLInputElement;
      if (input.name === 'display_name') this.name = input.value;
    },
  }));
});
