unsafe extern "C" {
    pub fn abs(x: i32) -> i32;
    fn labs(x: i64) -> i64;
    pub static errno: i32;
    fn printf(format: *const u8, ...) -> i32;
}

extern {
    fn bare();
}

extern "system" {
    fn GetTickCount() -> u32;
    type Handle;
    fn CloseHandle(handle: *mut Handle) -> i32;
}

pub fn between() {}

unsafe extern "C" {
    fn after_between();
}
