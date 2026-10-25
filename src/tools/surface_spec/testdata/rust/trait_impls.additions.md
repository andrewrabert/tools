# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

### trait_impls.rs
```rust
pub struct Wrapper<T>(T);

pub struct Raw(*mut u8);

impl<T: Display> Display for Wrapper<T>;

unsafe impl Send for Raw;

impl !Sync for Raw;

impl<T> From<T> for Wrapper<T>
where
    T: Clone,
;

impl<'a, T> Iterator for Wrapper<&'a [T]>;

impl Default for Raw;
```

## Deletions

No deletions.
