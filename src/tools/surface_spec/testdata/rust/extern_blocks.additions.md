# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

### extern_blocks.rs
```rust
unsafe extern "C" {
    pub fn abs(x: i32) -> i32;
    fn labs(x: i64) -> i64;
    fn printf(format: *const u8, ...) -> i32;
}

extern {
    fn bare();
}

extern "system" {
    fn GetTickCount() -> u32;
    fn CloseHandle(handle: *mut Handle) -> i32;
}

pub fn between();

unsafe extern "C" {
    fn after_between();
}
```

## Deletions

No deletions.
