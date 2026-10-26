pub fn outer(input: u32) -> u32 {
    struct Local {
        value: u32,
    }

    fn helper(local: &Local) -> u32 {
        fn innermost() -> u32 {
            1
        }
        local.value + innermost()
    }

    let closure = |x: u32| {
        fn in_closure(x: u32) -> u32 {
            x * 2
        }
        in_closure(x)
    };

    closure(helper(&Local { value: input }))
}

pub const LIMIT: usize = {
    enum Size {
        Small,
        Large,
    }

    fn pick(size: Size) -> usize {
        match size {
            Size::Small => 4,
            Size::Large => 8,
        }
    }

    pick(Size::Large)
};

pub struct Buffer {
    data: [u8; {
        const fn length() -> usize {
            16
        }
        length()
    }],
}

pub fn sized(values: [u8; { fn count() -> usize { 3 } count() }]) -> usize {
    values.len()
}

pub mod outer_mod {
    pub fn in_mod() {}

    pub struct InMod;

    mod inner_mod {
        pub(super) fn in_inner() {}

        pub mod innermost_mod {
            pub enum Deep {
                One,
            }

            impl Deep {
                pub fn get() -> Self {
                    Deep::One
                }
            }
        }
    }
}
