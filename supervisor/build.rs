fn main() {
    // option_env! 不被 cargo 自动跟踪: 不写这一行, 改了 CHAOS_BUILD_TIME 也不会重编,
    // 关于页会一直显示第一次编译进去的那个时间。
    println!("cargo:rerun-if-env-changed=CHAOS_BUILD_TIME");
}
