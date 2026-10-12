// Independent AFFiNE 0.27.4 organization fixture. Install yjs in scratch and set YJS_MODULE.
import { writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const Y = await import(process.env.YJS_MODULE ? pathToFileURL(process.env.YJS_MODULE).href : 'yjs');
const root = new Y.Doc();
const options = new Y.Array();
options.push([new Y.Map(Object.entries({ id: 'js-tag', value: 'Independent tag', color: '--affine-tag-blue', createDate: 1, updateDate: 1 }))]);
root.getMap('meta').set('properties', new Y.Map([
  ['tags', new Y.Map([['options', options]])],
  ['unrelated', 'preserve workspace properties'],
]));
const collections = new Y.Array();
collections.push([{ id: 'js-collection', name: 'Independent collection', allowList: ['fixture-page'], rules: { filters: [{ type: 'system', key: 'tags', method: 'include-all', value: 'js-tag' }] }, extra: 'preserve custom field' }]);
root.getMap('setting').set('collections', collections);
root.getMap('setting').set('unrelated', 'preserve workspace settings');
writeFileSync(new URL('./organization-root-update.bin', import.meta.url), Y.encodeStateAsUpdate(root));
const folders = new Y.Doc();
for (const row of [
  { id: 'js-folder', parentId: null, type: 'folder', data: 'Independent folder', index: 'a0' },
  { id: 'js-doc-link', parentId: 'js-folder', type: 'doc', data: 'fixture-page', index: 'a0' },
  { id: 'js-tag-link', parentId: 'js-folder', type: 'tag', data: 'js-tag', index: 'a1' },
  { id: 'js-collection-link', parentId: 'js-folder', type: 'collection', data: 'js-collection', index: 'a2' },
  { id: 'js-deleted', $$DELETED: true },
]) {
  const map = folders.getMap(row.id);
  for (const [key, value] of Object.entries(row)) map.set(key, value);
}
writeFileSync(new URL('./organization-folders.bin', import.meta.url), Y.encodeStateAsUpdate(folders));
const pins = new Y.Doc();
pins.getMap('js-collection').set('collectionId', 'js-collection');
pins.getMap('js-collection').set('index', 'a0');
writeFileSync(new URL('./organization-pins.bin', import.meta.url), Y.encodeStateAsUpdate(pins));
console.log('Generated independent JavaScript tag/collection/folder/pin fixtures');
