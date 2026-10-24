// Generate independent Yjs drawing fixtures plus a real CRC-checked PNG.
// YJS_MODULE may point to an existing yjs/dist/yjs.mjs installation.
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import { createHash } from 'node:crypto';
const Y = await import(process.env.YJS_MODULE || 'yjs');

function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const name = Buffer.from(type);
  const length = Buffer.alloc(4); length.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([name, data])));
  return Buffer.concat([length, name, data, crc]);
}
const header = Buffer.alloc(13);
header.writeUInt32BE(8, 0); header.writeUInt32BE(8, 4);
header[8] = 8; header[9] = 2; // 8-bit RGB, no interlacing.
const pixels = Buffer.concat(Array.from({ length: 8 }, (_, y) =>
  Buffer.from([0, ...Array.from({ length: 8 }, (_, x) => [x * 30, y * 30, 128]).flat()])));
const png = Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),
  chunk('IHDR', header), chunk('IDAT', deflateSync(pixels)), chunk('IEND', Buffer.alloc(0))]);
writeFileSync(new URL('sample-media.png', import.meta.url), png);
const attachment = Buffer.from('Independent native attachment 🌏\n');
writeFileSync(new URL('sample-attachment.txt', import.meta.url), attachment);
const sourceId = bytes => createHash('sha256').update(bytes).digest('base64url');

const doc = new Y.Doc();
const blocks = doc.getMap('blocks');
function block(id, flavour, version, children = [], props = {}) {
  const b = new Y.Map();
  b.set('sys:id', id); b.set('sys:flavour', flavour); b.set('sys:version', version);
  b.set('sys:children', new Y.Array());
  blocks.set(id, b); b.get('sys:children').push(children);
  for (const [key, value] of Object.entries(props)) b.set(`prop:${key}`, value);
  return b;
}
block('js-page', 'affine:page', 2, ['js-surface'], { title: new Y.Text('Independent drawing and media') });
const surface = block('js-surface', 'affine:surface', 5, ['js-frame', 'js-link', 'js-doc-link', 'js-image', 'js-file']);
const wrapper = new Y.Map(); wrapper.set('type', '$blocksuite:internal:native$');
wrapper.set('value', new Y.Map()); surface.set('prop:elements', wrapper);
const elements = wrapper.get('value');
function element(id, props) { const e = new Y.Map(); e.set('id', id); elements.set(id, e); for (const [key,value] of Object.entries(props)) e.set(key,value); }
element('js-pen', { type:'brush', index:'a0', seed:1, xywh:'[96,196,208,208]', rotate:0,
  points:[[4,4,0.2],[204,204,0.9]], lineWidth:8, color:{light:'#1e293b',dark:'#e2e8f0'} });
element('js-curve', { type:'connector', index:'a1', seed:1, source:{position:[500,100]}, target:{position:[650,300]},
  mode:2, text:new Y.Text('Editable independent curve'), frontEndpointStyle:'None', rearEndpointStyle:'None',
  stroke:{light:'#475569',dark:'#94a3b8'}, strokeWidth:2, strokeStyle:'solid', rough:false,
  labelDisplay:true, labelOffset:{distance:0.5,anchor:'center'}, labelConstraints:{hasMaxWidth:true,maxWidth:280},
  labelStyle:{color:{light:'#172554',dark:'#e2e8f0'},fontFamily:'blocksuite:surface:Inter',fontSize:16,fontStyle:'normal',fontWeight:'400',textAlign:'center'} });
const common = (index, xywh) => ({ index, xywh, lockedBySelf:false, rotate:0 });
block('js-frame','affine:frame',1,[],{ ...common('a2','[50,50,800,500]'),title:new Y.Text('Independent frame 🌏'),
  'background.light':'#eef2ff','background.dark':'#1e293b', 'childElementIds.js-pen':true,
  'childElementIds.js-curve':true,'childElementIds.js-image':true, presentationIndex:0 });
block('js-link','affine:bookmark',1,[],{...common('a3','[900,100,460,180]'),style:'horizontal',url:'https://affine.pro/',title:'AFFiNE',description:'External link',caption:null});
block('js-doc-link','affine:embed-linked-doc',1,[],{...common('a4','[900,350,460,180]'),style:'horizontal',pageId:'fixture-page',title:'Local document',description:'Same workspace',caption:null});
block('js-image','affine:image',1,[],{...common('a5','[100,500,80,80]'),sourceId:sourceId(png),width:8,height:8,size:png.length,caption:'Independent image'});
block('js-file','affine:attachment',1,[],{...common('a6','[900,600,460,80]'),sourceId:sourceId(attachment),name:'sample-attachment.txt',type:'text/plain',size:attachment.length,style:'horizontalThin',embed:false,caption:'Independent attachment',footnoteIdentifier:null});
writeFileSync(new URL('native-drawing.bin',import.meta.url),Y.encodeStateAsUpdate(doc));
console.log(JSON.stringify({ independentYjs:true,brushCount:1,curveCount:1,frameCount:1,linkCount:2,mediaCount:2,pngSize:png.length,imageSourceId:sourceId(png) }));
