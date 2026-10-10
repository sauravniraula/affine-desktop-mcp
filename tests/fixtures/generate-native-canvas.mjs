// Regenerate with JavaScript yjs (dev-only; never required by the Rust server).
// Install yjs in a scratch directory and set YJS_MODULE to its dist/yjs.mjs file.
import { writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const Y = await import(process.env.YJS_MODULE ? pathToFileURL(process.env.YJS_MODULE).href : 'yjs');
const doc = new Y.Doc();
const blocks = doc.getMap('blocks');
const block = (id, flavour, version, children = []) => {
  const map = new Y.Map(Object.entries({ 'sys:id': id, 'sys:flavour': flavour, 'sys:version': version }));
  map.set('sys:children', new Y.Array());
  blocks.set(id, map);
  map.get('sys:children').push(children);
  return map;
};
const page = block('native-page', 'affine:page', 2, ['native-surface']);
page.set('prop:title', new Y.Text('Independent native canvas'));
const surface = block('native-surface', 'affine:surface', 5);
const elements = new Y.Map();
surface.set('prop:elements', new Y.Map([['type', '$blocksuite:internal:native$'], ['value', elements]]));
for (const [id, index, text, shapeType, xywh] of [
  ['js-start', 'a0', 'Independent shape 🌏', 'ellipse', '[100,100,260,120]'],
  ['js-decision', 'a1', 'Continue?', 'diamond', '[100,320,260,120]'],
]) {
  elements.set(id, new Y.Map(Object.entries({ type: 'shape', index, seed: 1, text: new Y.Text(text),
    shapeType, xywh, radius: 0, filled: true, fillColor: { light: '#eef2ff', dark: '#1e293b' },
    strokeColor: { light: '#475569', dark: '#94a3b8' }, strokeWidth: 2, strokeStyle: 'solid',
    shapeStyle: 'General', color: { light: '#172554', dark: '#e2e8f0' },
    fontFamily: 'blocksuite:surface:Inter', fontSize: 20, fontStyle: 'normal', fontWeight: '400',
    textAlign: 'center', textHorizontalAlign: 'center', textVerticalAlign: 'center', textResizing: 1,
    rotate: 0, padding: [10, 20], maxWidth: false,
  })));
}
elements.set('js-arrow', new Y.Map(Object.entries({ type: 'connector', index: 'a2', seed: 1,
  source: { id: 'js-start', position: [0.5, 1] }, target: { id: 'js-decision', position: [0.5, 0] },
  mode: 1, stroke: '#475569', strokeWidth: 2, strokeStyle: 'solid', rough: false,
  frontEndpointStyle: 'None', rearEndpointStyle: 'Arrow', text: new Y.Text('Next step'),
  labelDisplay: true, labelOffset: { distance: 0.5, anchor: 'center' },
  labelConstraints: { hasMaxWidth: true, maxWidth: 280 },
  labelStyle: { color: '#172554', fontFamily: 'blocksuite:surface:Inter', fontSize: 16,
    fontStyle: 'normal', fontWeight: '400', textAlign: 'center' },
})));
writeFileSync(new URL('./native-canvas.bin', import.meta.url), Y.encodeStateAsUpdate(doc));
console.log('Generated native-canvas.bin with independent JavaScript Yjs');
