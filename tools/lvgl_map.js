#!/usr/bin/env node
// Scan a p67 vela_ap.bin for LVGL lv_obj_class_t descriptors.
// usage: node tools/lvgl_map.js <vela_ap.bin>
'use strict';
const fs = require('fs');
const b = fs.readFileSync(process.argv[2] || 'vela_ap.bin');
const DATA_BASE = 0x2C0C0000;
const OBJ_CLASS = 0x2CCE0308;
function strAt(va) {
  const o = va - DATA_BASE;
  if (o < 0 || o >= b.length) return null;
  let e = o;
  while (e < b.length && b[e] >= 32 && b[e] < 127) e++;
  if (e === o) return null;
  return b.toString('latin1', o, e);
}
const rows = [];
for (let va = 0x2CCE0000; va < 0x2CD00000; va += 4) {
  const o = va - DATA_BASE;
  if (o < 0 || o + 0x28 > b.length) continue;
  if (b.readUInt32LE(o) !== OBJ_CLASS) continue;
  const name = strAt(b.readUInt32LE(o + 0x14));
  if (!name || name.length > 24) continue;
  rows.push({ va, name, ctor: b.readUInt32LE(o + 4), event: b.readUInt32LE(o + 0xC), size: b.readUInt32LE(o + 0x20) });
}
rows.sort((a, c) => a.va - c.va);
for (const r of rows) {
  console.log([r.name, '0x' + r.va.toString(16).toUpperCase(), '0x' + r.ctor.toString(16).toUpperCase(),
    '0x' + r.event.toString(16).toUpperCase(), '0x' + r.size.toString(16)].join('\t'));
}
