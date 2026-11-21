pub fn plain(value: u32) -> u32 {
    value
}

pub async fn fetch(url: &str) -> Result<String, std::io::Error> {
    Ok(url.to_owned())
}

pub const fn square(x: u32) -> u32 {
    x * x
}

pub unsafe fn read(ptr: *const u8) -> u8 {
    unsafe { *ptr }
}

#[unsafe(no_mangle)]
pub extern "C" fn callback(code: i32) -> i32 {
    code
}

pub(crate) async unsafe fn both(ptr: *const u8) -> u8 {
    unsafe { *ptr }
}

const unsafe extern "C" fn all(x: i32) -> i32 {
    x
}

fn generic<'a, T, const N: usize>(items: &'a [T; N]) -> impl Iterator<Item = &'a T> + 'a
where
    T: Clone,
{
    items.iter()
}

fn diverges(_: u8, (left, right): (i32, i32)) -> ! {
    panic!("{left} {right}")
}
