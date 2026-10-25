# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

### traits.rs
```rust
pub trait Named {
    fn name(&self) -> String;
}

pub trait Shape: Named + Clone + Debug {
    type Unit;
    type Scaled: Shape<Unit = Self::Unit>;
    const SIDES: usize;
    const SCALE: f64 = 1.0;
    fn area(&self) -> f64;
    fn describe(&self) -> String;
    fn scale<F>(&self, factor: F) -> Self::Scaled
    where
        F: Into<f64> + Copy;
    fn largest<'a, I: IntoIterator<Item = &'a Self>>(shapes: I) -> Option<&'a Self>
    where
        Self: 'a;
}

pub(crate) unsafe trait RawAccess<T: ?Sized> {
    unsafe fn read(&self, offset: usize) -> T;
}
```

## Deletions

No deletions.
