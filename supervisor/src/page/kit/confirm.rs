//! 二次确认(两次点击模式)的统一实现。
//!
//! 语义: 第一击武装(界面文案换成"再点一次确认 X"), 第二击放行; 其它点击、
//! 重建、离开页面都要撤销。destructive 动作本体照旧走页面自己的登记-执行链。
//!
//! 用法: 页面持一个 `pub(super) static ARM: Arm = Arm::new();`
//!   - `ARM.click()` 返回 false = 这一击是武装击(换文案); true = 确认击(执行动作);
//!   - 任何取消路径调 `ARM.reset()`。
//! 系统消息框(lvx_page_msgbox)那套在 confirm_pop.rs(字体/图标页的删除确认), 不在这里。

use crate::*;

/// 两次点击确认的状态。实例放页面的 static; 内部只有一个 u32。
pub(crate) struct Arm {
    flag: core::cell::UnsafeCell<u32>,
}

// 页面 static 的 Arm 只在 UI 线程(事件派发/主节拍)访问, 与其它页面静态量同一约定。
unsafe impl Sync for Arm {}

impl Arm {
    pub(crate) const fn new() -> Self {
        Arm { flag: core::cell::UnsafeCell::new(0) }
    }

    /// 当前是否已武装(界面据此换文案)。
    pub(crate) fn armed(&self) -> bool {
        unsafe { read_volatile(self.flag.get()) != 0 }
    }

    /// 处理一次点击: false = 这次是武装击(换文案, 不执行); true = 确认击(执行动作,
    /// 状态自动归零 —— 执行后不需要再调 reset)。
    pub(crate) fn click(&self) -> bool {
        unsafe {
            if read_volatile(self.flag.get()) != 0 {
                write_volatile(self.flag.get(), 0);
                true
            } else {
                write_volatile(self.flag.get(), 1);
                false
            }
        }
    }

    /// 撤销武装(其它按钮被点、页面重建、离开页面时)。
    pub(crate) fn reset(&self) {
        unsafe { write_volatile(self.flag.get(), 0); }
    }
}
