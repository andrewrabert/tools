# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

### fn_qualifiers.rs
```rust
pub fn plain(value: u32) -> u32;

pub async fn fetch(url: &str) -> Result<String, std::io::Error>;

pub const fn square(x: u32) -> u32;

pub unsafe fn read(ptr: *const u8) -> u8;

#[unsafe(no_mangle)]
pub extern "C" fn callback(code: i32) -> i32;

pub(crate) async unsafe fn both(ptr: *const u8) -> u8;

const unsafe extern "C" fn all(x: i32) -> i32;

fn generic<'a, T, const N: usize>(items: &'a [T; N]) -> impl Iterator<Item = &'a T> + 'a
where
    T: Clone;

fn diverges(_: u8, (left, right): (i32, i32)) -> !;
```

## Deletions

No deletions.
