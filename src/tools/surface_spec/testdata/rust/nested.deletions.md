# Surface Spec: Rust
The set of `fn`, `struct`, `enum`, `trait`, and `impl` declarations the
resulting combined diff adds must equal the declarations under
`## Additions`, and the set it deletes must equal the declarations under
`## Deletions`.

## Additions

No additions.

## Deletions

### nested.rs
```rust
pub fn outer(input: u32) -> u32;

struct Local {
    value: u32,
}

fn helper(local: &Local) -> u32;

fn innermost() -> u32;

fn in_closure(x: u32) -> u32;

enum Size {
    Small,
    Large,
}

fn pick(size: Size) -> usize;

pub struct Buffer {
    data: [u8; { length() }],
}

const fn length() -> usize;

pub fn sized(values: [u8; { count() }]) -> usize;

fn count() -> usize;

pub fn in_mod();

pub struct InMod;

pub(super) fn in_inner();

pub enum Deep {
    One,
}

impl Deep {
    pub fn get() -> Self;
}
```
