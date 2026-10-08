export const STORAGE_KEY = 'actionlay.web.v1';
export const MAX_RECENT = 10;
const MAX_STORED_CHARS = 3 * 1024 * 1024;
export const defaultPreferences = () => ({ units: 'metric', maps: true, language: 'system', languageChosen: false, builtin: 'default', selected: null, codec: 'avc', mode: 'video' });
export function readState(storage) {
  const raw = storage.getItem(STORAGE_KEY);
  if (!raw) return { preferences: defaultPreferences(), layouts: [] };
  const state = JSON.parse(raw);
  if (state.version !== 1 || !Array.isArray(state.layouts) || state.layouts.some(x => !x || typeof x.id !== 'string' || typeof x.name !== 'string' || typeof x.bytes !== 'string')) throw new Error('Stored ActionLay data is invalid. Import a saved layout to recover it.');
  const preferences = { ...defaultPreferences(), ...state.preferences };
  // The first web build persisted English even when the user never chose a language.
  if (!preferences.languageChosen && preferences.language === 'en') preferences.language = 'system';
  return { preferences, layouts: state.layouts.slice(0, MAX_RECENT) };
}
export function writeState(storage, state) {
  const json = JSON.stringify({ version: 1, ...state });
  if (json.length > MAX_STORED_CHARS) throw new Error('Recent layouts exceed the local storage budget. Download the layout to keep a copy; remove older layouts to make room.');
  storage.setItem(STORAGE_KEY, json); // Atomic: old state survives quota/storage failures.
}
export function rememberLayout(state, entry) {
  return { ...state, preferences: { ...state.preferences, selected: entry.id, builtin: null }, layouts: [entry, ...state.layouts.filter(x => x.id !== entry.id)].slice(0, MAX_RECENT) };
}
export function toBase64(bytes) {
  let string = '';
  for (let i = 0; i < bytes.length; i += 8192) string += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(string);
}
export function fromBase64(text) { return Uint8Array.from(atob(text), c => c.charCodeAt(0)); }
