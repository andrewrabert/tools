# Surface Spec: Python
The set of `def` and `class` declarations the resulting combined diff
adds must equal the declarations under `## Additions`, and the set it
deletes must equal the declarations under `## Deletions`.

## Additions

### expressions.py
```python
def strings(
    a="plain",
    b='single "double" inside',
    c="it's",
    d="tab\there\nnewline \\ backslash",
    e="implicit concatenation mixed",
    f=r"raw\d+",
    g="unicode é é •",
    h=b'bytes\x00\xff "quoted"',
    i=b"implicit bytes",
    j="\x07 bell",
    k="""triple\nquoted""",
    l=R"raw upper \d",
    m=Rb"\d bytes",
    n=rb"it\'s raw",
    o='''triple \'single\' "double"''',
    p=r"""raw "quoted" triple""",
    q="escaped ' and \" quotes",
    r="unicode prefix",
    s=b"upper bytes",
    t="mixed" "quote" """styles""" r"raw",
    u="A A A A",
): ...


def fstrings(
    a=f"{value!r:>{width}}",
    b=f"{value=}",
    c=f"{ value = !s}",
    d=f"literal {{braces}} {x:.2f}",
    e=f"plain {x} tail",
    f=f"{d['key']}",
    g=f"{ {'a': 1}['a'] }",
    h=f"{(lambda: 1)()}",
    i=f"{x if y else z}",
    j=f"{f'{x}'}",
    k=f"{x!a}",
    l=t"hello {name!r}",
    m=f"{x}",
    n=rf"\d{x}",
    o=f'''triple {x} "quoted"''',
    p=f"{x}" "plain" f"{y}" r"raw",
    q=t"one {x}two {y}",
    r=f'escaped " {x}',
): ...


def numbers(
    a=255,
    b=1000000,
    c=15,
    d=10,
    e=1.5,
    f=10000000000.0,
    g=0.5,
    h=2.0j,
    i=0.0015j,
    j=0x_FFFF_FFFF_FFFF_FFFF_FFFF,
    k=123456789012345678901234567890,
    l=-1,
    m=1.0,
    n=255,
    o=100000.0,
    p=2.0j,
    q=15,
    r=1,
    s=1.05e-9,
    t=0.0,
): ...


def unions(
    a: int | None, b: "str | None", c: Optional[Union[int, str]]
) -> dict[str, list[int | None]]: ...


def subscripts(
    a: tuple[int, ...],
    b: tuple[()],
    c: tuple[int,],
    d: Callable[[int, str], None],
    e: Annotated[int, Gt(0)],
    f=x[1:2],
    g=x[::2],
    h=x[a:b, c],
    i=x[*ts,],
    j=x[lower + 1 : upper],
): ...


def lambdas(
    a=lambda: 0,
    b=lambda x, /, y=1, *args, z, **kw: x,
    c=sorted(items, key=lambda item: item[1]),
    d=(lambda: 1) if flag else lambda: 2,
    e=(lambda: 1)(),
    f=call(lambda: 1),
): ...


def comprehensions(
    a=[x for x in range(3)],
    b={x for x in range(3) if x},
    c={k: v for (k, v) in pairs if k if v},
    d=list(x for x in range(3)),
    e=sum((x for x in range(3)), start=0),
    g={**base, "key": 1},
    h=[x for x in a or b],
): ...


def precedence(
    a=(b + c) * d,
    b=b + c * d,
    c=b * c + d,
    d=b - (c - d),
    e=b - c - d,
    f=-(x**2),
    g=(-x) ** 2,
    h=x**-y,
    i=(x**y) ** z,
    j=x**y**z,
    k=not (a and b),
    l=not a and b,
    m=a or b and c,
    n=(a or b) and c,
    o=(a if b else c) if d else e,
    p=a if (b if c else d) else e,
    q=(a < b) < c,
    r=a < b < c,
    s=f(x := 1),
    t=f(k=(x := 1)),
    u=(1).real,
    v=(a, b)[0],
    w=(-1).real,
    x=--1,
    y=~(a | b),
    z=a | b & c ^ d,
    aa=(a | b) & c,
    bb=[*a, *(b or c)],
    cc=x[a if b else c],
    dd=(a := 1, 2),
    ee=a.b.c(d)[e],
    ff=(a + b).c,
    gg=not a == b,
    hh=a in b not in c is not d,
): ...


def generator():
    def uses_yield(a=(yield), b=(yield x), c=(yield from items)): ...


async def coroutine():
    def uses_await(
        a=await x, b=(await x).y, c=-await x, d=[y async for y in source()]
    ): ...
```

## Deletions

No deletions.
