#!/usr/bin/env node
// ko_slim.js — supervisor .ko 体积后处理（S1：局部符号名置空）。
//
// 原理：NuttX modlib 做重定位只认 .symtab 的条目下标，局部符号的名字加载器用不到；
// 本模块未定义符号为 0，.strtab 里保留下来的名字只给「全局/弱符号导出」和调试看。
// 所以把 STB_LOCAL 且已定义符号的 st_name 置 0，只保留 GLOBAL/WEAK/未定义符号的名字。
//
// 用法: node tools/ko_slim.js <in.ko> [out.ko]
//   out 省略时原地覆盖。
//
// 退出码非 0 时不会写文件。

'use strict';
const fs = require('fs');

const files = process.argv.slice(2).filter((a) => !a.startsWith('--'));
const src = files[0];
const dst = files[1] || src;
if (!src) {
  console.error('usage: node tools/ko_slim.js <in.ko> [out.ko]');
  process.exit(2);
}

const b = fs.readFileSync(src);
if (b.toString('latin1', 0, 4) !== '\x7fELF') throw new Error('not ELF');
if (b.readUInt16LE(16) !== 1) throw new Error('not ET_REL');

const shoff = b.readUInt32LE(0x20);
const shentsize = b.readUInt16LE(0x2e);
const shnum = b.readUInt16LE(0x30);
const shstrndx = b.readUInt16LE(0x32);
const shstrOff = b.readUInt32LE(shoff + shstrndx * shentsize + 0x10);
const shstrSize = b.readUInt32LE(shoff + shstrndx * shentsize + 0x14);
const shstr = b.subarray(shstrOff, shstrOff + shstrSize);
const nm = (o) => {
  let s = '';
  for (let i = o; i < shstr.length; i++) {
    if (!shstr[i]) break;
    s += String.fromCharCode(shstr[i]);
  }
  return s;
};

const secs = [];
for (let i = 0; i < shnum; i++) {
  const o = shoff + i * shentsize;
  const s = {
    name: nm(b.readUInt32LE(o)),
    type: b.readUInt32LE(o + 4),
    flags: b.readUInt32LE(o + 8),
    addr: b.readUInt32LE(o + 12),
    off: b.readUInt32LE(o + 16),
    size: b.readUInt32LE(o + 20),
    link: b.readUInt32LE(o + 24),
    info: b.readUInt32LE(o + 28),
    align: b.readUInt32LE(o + 32) || 1,
    entsize: b.readUInt32LE(o + 36),
  };
  s.raw = b.subarray(s.off, s.off + s.size);
  secs.push(s);
}
const byName = (n) => secs.find((s) => s.name === n);
const sym = byName('.symtab');
const strt = byName('.strtab');
const shstrSec = byName('.shstrtab');
if (!sym || !strt || !shstrSec) throw new Error('missing .symtab/.strtab/.shstrtab');

// ---- 1. 局部符号名置空 ---------------------------------------------------
const symBuf = Buffer.from(sym.raw);
const n = symBuf.length / 16;
let blanked = 0;
for (let i = 0; i < n; i++) {
  const o = i * 16;
  const bind = symBuf.readUInt8(o + 12) >> 4;
  const shndx = symBuf.readUInt16LE(o + 14);
  if (shndx !== 0 && bind === 0 && symBuf.readUInt32LE(o) !== 0) {
    symBuf.writeUInt32LE(0, o);
    blanked++;
  }
}

// ---- 2. 重建 .strtab（只保留仍被引用的名字，偏移 0 恒为 NUL）------------
let newStr = Buffer.from([0]);
const remap = new Map();
for (let i = 0; i < n; i++) {
  const o = i * 16;
  const oldOff = symBuf.readUInt32LE(o);
  if (oldOff === 0) continue;
  if (!remap.has(oldOff)) {
    let e = oldOff;
    while (strt.raw[e]) e++;
    remap.set(oldOff, newStr.length);
    newStr = Buffer.concat([newStr, strt.raw.subarray(oldOff, e), Buffer.from([0])]);
  }
  symBuf.writeUInt32LE(remap.get(oldOff), o);
}
sym.raw = symBuf;
sym.size = symBuf.length;

// ---- 3. 重排数据块（保持节下标不变，避免 sh_link/st_shndx 失效）---------
const ehdr = Buffer.from(b.subarray(0, 0x34));
const parts = [];
let pos = 0x34;
const place = (s, data) => {
  const a = Math.max(s.align, 1);
  if (pos % a) pos += a - (pos % a);
  s.off = pos;
  s.size = data.length;
  parts.push({ off: pos, data });
  pos += data.length;
};
for (const s of secs) {
  if (s.type === 8 /* SHT_NOBITS */) continue; // bss 不占文件
  if (s === shstrSec || s === strt) continue; // 最后单独放
  if (s.size === 0) {
    s.off = 0;
    continue;
  }
  place(s, Buffer.from(s.raw));
}

// shstrtab 重建（节名表），strtab 紧随其后、放在文件最末（verify 脚本会把
// strtab 之后的内容当作名字表读取，所以它后面只能跟节头表）。
const nameBuf = [Buffer.from([0])];
const nameMap = {};
for (const s of secs) {
  if (!s.name) continue;
  nameMap[s.name] = nameBuf.reduce((a, x) => a + x.length, 0);
  nameBuf.push(Buffer.from(s.name + '\0'));
}
place(shstrSec, Buffer.concat(nameBuf));
place(strt, newStr);

// ---- 4. 节头表 -----------------------------------------------------------
const newShoff = pos;
const shdr = Buffer.alloc(shnum * shentsize);
for (let i = 0; i < shnum; i++) {
  const s = secs[i];
  const o = i * shentsize;
  shdr.writeUInt32LE(s.name ? nameMap[s.name] || 0 : 0, o);
  shdr.writeUInt32LE(s.type, o + 4);
  shdr.writeUInt32LE(s.flags, o + 8);
  shdr.writeUInt32LE(s.addr, o + 12);
  shdr.writeUInt32LE(s.off, o + 16);
  shdr.writeUInt32LE(s.size, o + 20);
  shdr.writeUInt32LE(s.link, o + 24);
  shdr.writeUInt32LE(s.info, o + 28);
  shdr.writeUInt32LE(s.align, o + 32);
  shdr.writeUInt32LE(s.entsize, o + 36);
}

const out = Buffer.alloc(pos + shdr.length);
ehdr.copy(out, 0);
for (const p of parts) p.data.copy(out, p.off);
shdr.copy(out, pos);
out.writeUInt32LE(newShoff, 0x20);

fs.writeFileSync(dst, out);
console.log(`ko_slim: ${b.length} -> ${out.length} B (局部符号名置空 ${blanked}/${n} 个)`);
