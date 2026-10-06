//! 通透背景使用的图像和快照接口。
use core::mem::transmute;
use super::Obj;

/// 创建图像，显式清空初始化配置参数。
pub unsafe extern "C" fn background_image_create(parent: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32) -> Obj = transmute(0x0C58_8D59usize);
    f(parent, 0)
}

/// 设置变量图像描述符或资源路径。
pub unsafe extern "C" fn background_image_set_source(image: Obj, source: u32) {
    let f: unsafe extern "C" fn(Obj, u32) = transmute(0x0C58_7FA1usize);
    f(image, source)
}

/// 设置横纵缩放，256 为一倍。
pub unsafe extern "C" fn background_image_set_scale(image: Obj, x: u32, y: u32) {
    let f: unsafe extern "C" fn(Obj, u32, u32) = transmute(0x1C06_C971usize);
    f(image, x, y)
}

/// 固定在父对象底层，不参与布局或随父对象内容滚动。
pub unsafe extern "C" fn background_image_set_floating(image: Obj) {
    let f: unsafe extern "C" fn(Obj, u32) = transmute(0x0C58_8961usize);
    f(image, 0x40000);
}

/// 设置图像变换枢轴。
pub unsafe extern "C" fn background_image_set_pivot(image: Obj, x: i32, y: i32) {
    let f: unsafe extern "C" fn(Obj, i32, i32) = transmute(0x0C58_7BC1usize);
    f(image, x, y)
}

/// 将背景移到父对象子表的首位。
pub unsafe extern "C" fn background_image_move_back(image: Obj) {
    let f: unsafe extern "C" fn(Obj, i32) = transmute(0x0C58_9551usize);
    f(image, 0)
}

/// 移除对象标志。
pub unsafe extern "C" fn background_clear_flags(obj: Obj, flags: u32) {
    let f: unsafe extern "C" fn(Obj, u32) = transmute(0x0C58_91B1usize);
    f(obj, flags)
}

/// 写入对象局部底色透明度。
pub unsafe extern "C" fn background_set_opa(obj: Obj, opacity: u32) {
    let f: unsafe extern "C" fn(Obj, u32, u32) = transmute(0x0C58_83F9usize);
    f(obj, opacity, 0)
}

/// 捕获表盘并借出共享绘图缓冲，失败返回零。
pub unsafe extern "C" fn background_snapshot_acquire() -> u32 {
    let f: unsafe extern "C" fn() -> u32 = transmute(0x0C5F_3771usize);
    f()
}

/// 归还共享绘图缓冲，不释放池内存。
pub unsafe extern "C" fn background_snapshot_release(buffer: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0C4B_6B35usize);
    f(buffer)
}
