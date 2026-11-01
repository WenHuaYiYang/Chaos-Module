#!/usr/bin/env node
// p67_pack.js — p67 容器打包/解析（p67_pack.py 的 node 移植，构建机没 python3 时用）。
//
// 格式（来自官方 Canopus 管理器安装包）:
//   0x000 主头 0xA8
//   0x0A8 11 x (u32 count, u32 offset)
//   0x100 风格名 72B
//   0x148 基础定义表 16B
//   0x158 资源定义表 N*16B
//   之后  根属性 16B
//   之后  预览块（原样字节）
//   之后  资源块，首尾相接，无对齐
// 资源块: u32 (dataLen | nameLen<<24) + 16B 0 + ASCII 名 + 数据。
//
// 用法:
//   node tools/p67_pack.js inspect <file.bin>
//   node tools/p67_pack.js build <spec.json> <out.bin>
//   node tools/p67_pack.js repack <src.bin> <out.bin> <name>=<path> [<name>=<path> ...]
//     （repack: 用已有容器的其它资源 + 替换/新增指定资源，重打一份）

'use strict';
const fs = require('fs');
const path = require('path');

const MAGIC = 0x1234a55a;
const VERSION = 0x00010007;
const HDR = 0xa8;
const PAIR_OFF = 0xa8;
const PAIRS = 11;
const BASE_OFF = 0x148;
const SCRIPT_OFF = 0x158;
const DEF_SIZE = 16;

function u32(v) {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(v >>> 0, 0);
  return b;
}
function u16(v) {
  const b = Buffer.alloc(2);
  b.writeUInt16LE(v & 0xffff, 0);
  return b;
}
function pad(data, n) {
  const out = Buffer.alloc(n);
  data.copy(out, 0, 0, Math.min(data.length, n));
  return out;
}
function def(index, typ, offset, length) {
  const b = Buffer.alloc(DEF_SIZE);
  u16(index).copy(b, 0);
  b[3] = typ;
  u32(offset).copy(b, 8);
  u32(length).copy(b, 12);
  return b;
}

function readBlock(buf, off) {
  const packed = buf.readUInt32LE(off);
  const dataLen = packed & 0xffffff;
  const nameLen = packed >>> 24;
  const name = buf.toString('latin1', off + 20, off + 20 + nameLen);
  const dataOff = off + 20 + nameLen;
  return {
    name,
    data: buf.subarray(dataOff, dataOff + dataLen),
    offset: off,
    length: 20 + nameLen + dataLen,
  };
}

function inspect(file) {
  const buf = fs.readFileSync(file);
  if (buf.length < HDR) throw new Error('文件太短');
  const pairs = [];
  for (let i = 0; i < PAIRS; i++) {
    pairs.push([buf.readUInt32LE(PAIR_OFF + i * 8), buf.readUInt32LE(PAIR_OFF + i * 8 + 4)]);
  }
  const [scriptCount, scriptOff] = pairs[6];
  const files = [];
  for (let i = 0; i < scriptCount; i++) {
    const e = scriptOff + i * DEF_SIZE;
    const index = buf.readUInt16LE(e);
    const typ = buf[e + 3];
    const off = buf.readUInt32LE(e + 8);
    const length = buf.readUInt32LE(e + 12);
    const blk = readBlock(buf, off);
    files.push({ index, type: typ, name: blk.name, size: blk.data.length, offset: off, blockLength: length });
  }
  const previewOff = buf.readUInt32LE(0x20);
  const width = buf.readUInt16LE(previewOff + 4);
  const height = buf.readUInt16LE(previewOff + 6);
  const id = buf.toString('latin1', 0x28, 0x34).split('\0')[0];
  const name = buf.toString('utf8', 0x68, 0xa4).split('\0')[0];
  const styleName = buf.toString('utf8', 0x100, 0x148).split('\0')[0];
  return {
    magic: buf.readUInt32LE(0),
    version: buf.readUInt32LE(4),
    id,
    name,
    styleName,
    preview: [width, height],
    previewOff,
    // 预览块长度 = 第一个资源块的偏移 − 预览偏移（不是 width*height*2，那是像素尺寸）
    previewData: buf.subarray(previewOff, files.length ? files[0].offset : buf.length),
    files,
    size: buf.length,
  };
}

function build(spec, out) {
  const preview = fs.readFileSync(spec.preview);
  const entries = spec.files.map((it) => {
    const name = Buffer.from(it.name, 'latin1');
    if (name.length > 0xff) throw new Error(`资源名太长: ${it.name}`);
    const data = fs.readFileSync(it.path);
    if (data.length > 0xffffff) throw new Error(`资源太大: ${it.name}`);
    return { name, data };
  });

  const n = entries.length;
  const propsOff = SCRIPT_OFF + n * DEF_SIZE;
  const previewOff = propsOff + DEF_SIZE;
  let cursor = previewOff + preview.length;

  const blocks = [];
  for (const { name, data } of entries) {
    const block = Buffer.concat([u32(data.length | (name.length << 24)), Buffer.alloc(16), name, data]);
    blocks.push({ name, block, off: cursor });
    cursor += block.length;
  }

  const buf = Buffer.alloc(cursor);
  u32(MAGIC).copy(buf, 0);
  u32(VERSION).copy(buf, 4);
  u32(0x00000800).copy(buf, 0x10);
  u32(0x00010000).copy(buf, 0x14);
  u16(1).copy(buf, 0x1c);
  u16(0).copy(buf, 0x1e);
  u32(previewOff).copy(buf, 0x20);
  pad(Buffer.from(spec.id || '979820260926', 'latin1'), 12).copy(buf, 0x28);
  pad(Buffer.from(spec.name || 'Chaos 安装器', 'utf8'), 60).copy(buf, 0x68);
  pad(Buffer.from(spec.styleName || spec.style_name || spec.name || 'Chaos 安装器', 'utf8'), 72).copy(buf, 0x100);

  const pairs = [
    [0x80000000, previewOff],
    [1, BASE_OFF],
    [0, SCRIPT_OFF],
    [0, SCRIPT_OFF],
    [0, SCRIPT_OFF],
    [0, SCRIPT_OFF],
    [n, SCRIPT_OFF],
    [0, propsOff],
    [0, propsOff],
    [0, propsOff],
    [0, propsOff],
  ];
  pairs.forEach(([cnt, off], i) => {
    u32(cnt).copy(buf, PAIR_OFF + i * 8);
    u32(off).copy(buf, PAIR_OFF + i * 8 + 4);
  });

  def(0, 0, propsOff, DEF_SIZE).copy(buf, BASE_OFF);
  blocks.forEach(({ block, off }, i) => {
    def(i, 5, off, block.length).copy(buf, SCRIPT_OFF + i * DEF_SIZE);
  });

  const entryIndex = spec.entry_index || 0;
  u16(entryIndex).copy(buf, propsOff);
  buf[propsOff + 2] = 0;
  buf[propsOff + 3] = 5;

  preview.copy(buf, previewOff);
  for (const { block, off } of blocks) block.copy(buf, off);

  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, buf);
  return { size: buf.length, previewOff, blocks: entries.map((e) => [e.name.toString('latin1'), e.data.length]) };
}

function cmdInspect(file) {
  const info = inspect(file);
  const out = {
    magic: '0x' + info.magic.toString(16),
    version: '0x' + info.version.toString(16),
    id: info.id,
    name: info.name,
    styleName: info.styleName,
    preview: info.preview,
    files: info.files.map((f) => ({ name: f.name, size: f.size, type: f.type })),
    size: info.size,
  };
  console.log(JSON.stringify(out, null, 2));
  return info;
}

// repack: 以已有容器为底，保留其全部资源（含 lua/图标/fetch/capatch/ca.pem），
// 用 name=path 覆盖或追加资源，重新打包。
function cmdRepack(src, out, overrides) {
  const info = inspect(src);
  const keep = info.files.map((f) => ({ name: f.name, data: readBlock(fs.readFileSync(src), f.offset).data }));
  const byName = new Map(keep.map((e) => [e.name, e]));
  const order = keep.map((e) => e.name);
  for (const [name, file] of overrides) {
    const data = fs.readFileSync(file);
    if (byName.has(name)) byName.get(name).data = data;
    else {
      byName.set(name, { name, data });
      order.push(name);
    }
    console.log(`repack: ${name} <- ${file} (${data.length} B)`);
  }
  // build() 只吃文件路径，先落到临时目录
  const tmp = path.join(path.dirname(out), '.repack_tmp');
  fs.mkdirSync(tmp, { recursive: true });
  const files = order.map((name, i) => {
    const p = path.join(tmp, `${i}_${path.basename(name).replace(/[^\w.\-]/g, '_')}`);
    fs.writeFileSync(p, byName.get(name).data);
    return { name, path: p };
  });
  const previewPath = path.join(tmp, 'preview.block');
  fs.writeFileSync(previewPath, info.previewData);
  const spec = {
    id: info.id,
    name: info.name,
    styleName: info.styleName,
    preview: previewPath,
    files,
  };
  const res = build(spec, out);
  fs.rmSync(tmp, { recursive: true, force: true });
  console.log(JSON.stringify({ size: res.size, blocks: res.blocks.map(([n, s]) => `${n} ${s}`) }, null, 2));
  return res;
}

function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  if (cmd === 'inspect') {
    cmdInspect(rest[0]);
  } else if (cmd === 'build') {
    const spec = JSON.parse(fs.readFileSync(rest[0], 'utf8'));
    const res = build(spec, rest[1]);
    console.log(JSON.stringify(res, null, 2));
  } else if (cmd === 'repack') {
    const [src, out, ...pairs] = rest;
    const overrides = pairs.map((p) => {
      const i = p.indexOf('=');
      if (i < 0) throw new Error('用法: name=path');
      return [p.slice(0, i), p.slice(i + 1)];
    });
    cmdRepack(src, out, overrides);
  } else {
    console.error('usage: p67_pack.js inspect <bin> | build <spec.json> <out> | repack <src> <out> name=path...');
    process.exit(2);
  }
}

module.exports = { MAGIC, VERSION, inspect, build, readBlock };
if (require.main === module) main();
