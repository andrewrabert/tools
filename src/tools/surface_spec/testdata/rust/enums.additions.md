# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

### enums.rs
```rust
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

pub enum Shape {
    Circle(f64),
    Rectangle { width: f64, height: f64 },
    Point,
}

#[repr(u8)]
enum Status {
    Ok = 0,
    NotFound = 4,
    Failed = 1 << 3,
}

pub enum Tree<'a, T>
where
    T: PartialOrd,
{
    Leaf(&'a T),
    Node { left: Box<Tree<'a, T>>, right: Box<Tree<'a, T>> },
    Empty,
}
```

## Deletions

No deletions.
