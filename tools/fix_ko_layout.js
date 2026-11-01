#!/usr/bin/env node
// fix_ko_layout.js — fix_ko_layout.py 的 node 移植（构建环境没有 python3 时用）。
//
// 把 chaos_sup.ko 内容节（.text/.rodata/.data/.bss）的 sh_addr 重写为连续布局：
//   - .text 从 0 开始
//   - .rodata 8 字节对齐，.data/.bss 4 字节对齐
//   - .init_array/.fini_array/.ARM.exidx 保持 addr=0
//   - .init_array/.fini_array flags 补 WA(3)
//
// 用法: node fix_ko_layout.js <in.ko> [out.ko]（默认原地覆盖）

'use strict';
const fs = require('fs');

function fix(path, out) {
  out = out || path;
  const d = fs.readFileSync(path);
  const eShnum = d.readUInt16LE(0x30);
  const eShstrndx = d.readUInt16LE(0x32);
  const eShoff = d.readUInt32LE(32);

  const secs = [];
  for (let i = 0; i < eShnum; i++) {
    const off = eShoff + i * 40;
    secs.push({
      i, off,
      name_off: d.readUInt32LE(off),
      type: d.readUInt32LE(off + 4),
      flags: d.readUInt32LE(off + 8),
      addr: d.readUInt32LE(off + 12),
      offset: d.readUInt32LE(off + 16),
      size: d.readUInt32LE(off + 20),
      align: d.readUInt32LE(off + 32),
    });
  }
  const shstr = secs[eShstrndx];
  const nameAt = (n) => {
    let end = d.indexOf(0, shstr.offset + n);
    return d.toString('latin1', shstr.offset + n, end);
  };
  for (const s of secs) s.name = nameAt(s.name_off);

  const CONTENT = new Set(['.text', '.rodata', '.data', '.bss']);
  let cur = 0;
  const changed = [];
  for (const s of secs) {
    if (!CONTENT.has(s.name) || s.size === 0) continue;
    const align = s.name === '.rodata' ? 8 : 4;
    cur = (cur + align - 1) & ~(align - 1);
    const newAddr = cur;
    cur = newAddr + s.size;
    if (newAddr !== s.addr) {
      d.writeUInt32LE(newAddr, s.off + 12);
      changed.push([s.name, s.addr, newAddr]);
    }
    if (s.name === '.rodata' && s.align !== 8) {
      d.writeUInt32LE(8, s.off + 32);
      changed.push([s.name + '.align', s.align, 8]);
    }
  }
  for (const s of secs) {
    if ((s.name === '.init_array' || s.name === '.fini_array') && (s.flags & 1) === 0) {
      d.writeUInt32LE(s.flags | 1, s.off + 8);
      changed.push([s.name + '.flags', s.flags, s.flags | 1]);
    }
  }

  fs.writeFileSync(out, d);
  console.log(`fixed ${out}: ${changed.length} fields`);
  for (const [name, a, b] of changed) console.log(`  ${name}: 0x${a.toString(16).toUpperCase()} -> 0x${b.toString(16).toUpperCase()}`);
  console.log('\nname            type        addr      size');
  for (const s of secs) {
    if (s.type === 0) continue;
    const addr = d.readUInt32LE(s.off + 12);
    console.log(`${s.name.padEnd(16)}${String(s.type).padEnd(12)}0x${addr.toString(16).toUpperCase().padStart(6, '0')}0x${s.size.toString(16).toUpperCase().padStart(6, '0')}`);
  }
}

const args = process.argv.slice(2);
if (args.length < 1) {
  console.error('usage: node fix_ko_layout.js <in.ko> [out.ko]');
  process.exit(2);
}
fix(args[0], args[1]);
