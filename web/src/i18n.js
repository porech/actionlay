const catalogues = import.meta.glob('../../crates/app/locales/*.json');
const webCatalogues = import.meta.glob('../locales/*.json');
const supported = new Set(Object.keys(catalogues).map(path => path.split('/').pop().replace('.json', '')));
let shared = {};
let web = {};
let current = 'en';
export function resolveLanguage(choice, languages = navigator.languages) {
  const candidates = choice && choice !== 'system' ? [choice] : languages;
  for (const language of candidates) {
    const code = language.replaceAll('_', '-').toLowerCase();
    const exact = [...supported].find(x => x.toLowerCase() === code);
    if (exact) return exact;
    const parts = code.split('-');
    let primary = parts[0];
    if (primary === 'zh') primary = parts.some(p => ['hant','tw','hk','mo'].includes(p)) ? 'zh-Hant' : 'zh-Hans';
    if (primary === 'pt') primary = parts.includes('br') ? 'pt-BR' : 'pt-PT';
    if (primary === 'no') primary = 'nb';
    if (supported.has(primary)) return primary;
  }
  return 'en';
}
export async function setLanguage(choice) {
  current = resolveLanguage(choice);
  shared = (await catalogues[`../../crates/app/locales/${current}.json`]()).default;
  const extra = webCatalogues[`../locales/${current}.json`];
  web = extra ? (await extra()).default : {};
  document.documentElement.lang = current;
  document.documentElement.dir = ['ar','he'].includes(current) ? 'rtl' : 'ltr';
  for (const element of document.querySelectorAll('[data-i18n]')) element.textContent = t(element.dataset.i18n);
  for (const element of document.querySelectorAll('[data-i18n-label]')) element.setAttribute('aria-label', t(element.dataset.i18nLabel));
  return current;
}
export function t(source, parameters = {}) {
  // Desktop catalogues use numbered placeholders for formatted translations.
  let text = web[source] ?? shared[source] ?? source;
  Object.entries(parameters).forEach(([name,value], index) => {
    text = text.replaceAll(`{${name}}`, String(value)).replaceAll(`{${index}}`, String(value));
  });
  return text;
}
