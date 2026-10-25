# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

No additions.

## Deletions

### inherent_impls.rs
```rust
pub struct Stack<T> {
    items: Vec<T>,
}

impl<T: Clone> Stack<T> {
    pub fn new() -> Self;
    pub fn push(&mut self, item: T);
    fn peek(&self) -> Option<&T>;
}

impl Stack<u8> {
    pub fn sum(&self) -> u32;
}

impl<T> Stack<T>
where
    T: Default + Clone,
{
    pub(crate) fn fill(count: usize) -> Self;
    pub fn len<'a>(&'a self) -> usize;
}
```
