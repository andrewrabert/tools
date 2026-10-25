# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

### structs.rs
```rust
#[derive(Debug, Clone)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    label: String,
}

pub struct Pair(pub u8, u16);

pub struct Unit;

pub(crate) struct Borrowed<'a, T> {
    pub value: &'a T,
    count: usize,
}

pub struct Matrix<T, const ROWS: usize, const COLS: usize> {
    cells: [[T; COLS]; ROWS],
}

struct Bounded<'a, K, V = ()>
where
    K: Ord + Debug,
    V: 'a,
{
    key: K,
    value: &'a V,
}

pub struct Wrapped<T: Clone>(
    T,
)
where
    T: Default;
```

## Deletions

No deletions.
