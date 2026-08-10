import { JSDOM } from 'jsdom';
const dom = new JSDOM('<!doctype html><div id="t" style="width:800px;height:600px"></div>', { pretendToBeVisual: true });
for (const k of ['window','document','navigator','HTMLElement','Element','Node','getComputedStyle','MutationObserver','ResizeObserver','requestAnimationFrame','cancelAnimationFrame','CustomEvent','Event','KeyboardEvent'])
  if (!(k in globalThis)) globalThis[k] = dom.window[k] ?? dom.window;
globalThis.window = dom.window; globalThis.document = dom.window.document;
globalThis.ResizeObserver ??= class { observe(){} unobserve(){} disconnect(){} };
globalThis.matchMedia ??= () => ({ matches:false, addListener(){}, removeListener(){}, addEventListener(){}, removeEventListener(){} });
dom.window.matchMedia ??= globalThis.matchMedia;

const X = await import('@xterm/xterm'); const { Terminal } = X.default ?? X;
const F = await import('@xterm/addon-fit'); const { FitAddon } = F.default ?? F;
const U = await import('@xterm/addon-unicode11'); const { Unicode11Addon } = U.default ?? U;

const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
const fit = new FitAddon(); const uni = new Unicode11Addon();
term.loadAddon(fit); term.loadAddon(uni);
term.open(document.getElementById('t'));
term.unicode.activeVersion = '11';

console.log('xterm cols/rows      :', term.cols, term.rows);
console.log('unicode versions     :', term.unicode.versions.join(','));
console.log('unicode active       :', term.unicode.activeVersion);
const wcs = term._core?.unicodeService ?? term.unicode;
console.log('fit.proposeDimensions:', JSON.stringify(fit.proposeDimensions()));
// Width table check: CJK must be 2 cells, combining mark 0.
const svc = term._core._unicodeService ?? term._core.unicodeService;
if (svc) {
  console.log('wcwidth 漢 (want 2)   :', svc.wcwidth(0x6f22));
  console.log('wcwidth a  (want 1)   :', svc.wcwidth(0x61));
  console.log('wcwidth U+0301 (want 0):', svc.wcwidth(0x301));
  console.log('wcwidth 🎉 (want 2)   :', svc.wcwidth(0x1f389));
}
term.write('hello \u001b[31mred\u001b[0m 漢字\r\n', () => {
  console.log('write callback fired  : yes');
  console.log('RESULT: addons load and drive xterm 6.0.0');
});
