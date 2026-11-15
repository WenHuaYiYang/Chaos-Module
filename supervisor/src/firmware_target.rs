//! Rust/C 共享的精确固件 symbol；由构建脚本校验摘要后导入。
#![allow(dead_code)]
include!(concat!(env!("OUT_DIR"), "/firmware.rs"));
