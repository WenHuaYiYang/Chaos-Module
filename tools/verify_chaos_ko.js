#!/usr/bin/env node
// verify_chaos_ko.js — verify_chaos_ko.py 的 node 移植。
// 校验: ET_REL / ARM / e_entry / 无未定义符号 / .init_array / 重定位类型。
//
// 用法: node verify_chaos_ko.js <chaos_sup.ko>

'use strict';
const fs = require('fs');

const path = process.argv[2];
if (!path) { console.error('usage: node verify_chaos_ko.js <ko>'); process.exit(2); }
const data = fs.readFileSync(path);

if (data.toString('latin1', 0, 4) !== '\x7fELF') throw new Error('not ELF');
if (data.readUInt16LE(16) !== 1) throw new Error('not ET_REL');
if (data.readUInt16LE(18) !== 40) throw new Error('not ARM');
const eEntry = data.readUInt32LE(24);
console.log(`ELF OK: ${path} (${data.length}B) e_entry=0x${eEntry.toString(16).toUpperCase()}`);

const shoff = data.readUInt32LE(32);
const shentsize = data.readUInt16LE(46);
const shnum = data.readUInt16LE(48);
const shstrndx = data.readUInt16LE(50);
const shstrOff = data.readUInt32LE(shoff + shstrndx * shentsize + 16);

const secs = {};
for (let i = 0; i < shnum; i++) {
  const e = shoff + i * shentsize;
  const nm = data.readUInt32LE(e);
  let end = data.indexOf(0, shstrOff + nm);
  const name = data.toString('latin1', shstrOff + nm, end);
  secs[name] = {
    type: data.readUInt32LE(e + 4),
    flags: data.readUInt32LE(e + 8),
    off: data.readUInt32LE(e + 16),
    size: data.readUInt32LE(e + 20),
  };
}

// 未定义符号
const strtabSec = secs['.strtab'];
const strtab = strtabSec ? data.subarray(strtabSec.off) : Buffer.alloc(0);
const undef = [];
if (secs['.symtab']) {
  const { off, size } = secs['.symtab'];
  for (let j = 0; j < size / 16; j++) {
    const se = off + j * 16;
    const sn = data.readUInt32LE(se);
    const shndx = data.readUInt16LE(se + 14);
    if (shndx === 0) {
      let end = strtab.indexOf(0, sn);
      const nm2 = strtab.toString('latin1', sn, end);
      if (nm2 && !nm2.startsWith('__') && nm2 !== 'rust_begin_unwind') undef.push(nm2);
    }
  }
}
console.log('未定义符号:', undef.length ? undef : '无 (OK)');

const init = Object.keys(secs).filter((n) => n.startsWith('.init_array'));
console.log('init_array:', init.length ? init.map((n) => [n, secs[n].size]) : '无!');

const rel = Object.keys(secs).filter((n) => n.startsWith('.rel'));
const types = {};
for (const n of rel) {
  const { off, size } = secs[n];
  for (let j = 0; j < size / 8; j++) {
    const rInfo = data.readUInt32LE(off + j * 8 + 4);
    const t = rInfo & 0xff;
    types[t] = (types[t] || 0) + 1;
  }
}
console.log('重定位类型:', types);
