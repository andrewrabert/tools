def plain():
    pass


def positional(a, b, /, c, d=1, *args, e, f=2, **kwargs):
    return a


def annotated(a: int, b: str = "x", *args: int, flag: bool = False, **kwargs: object) -> None:
    return None


def keyword_only(*, key: str) -> str:
    return key


def positional_only(a: int, /) -> int:
    return a


async def fetch(url: str, timeout: float = 1.5) -> bytes:
    return b""


@decorator
@decorator_factory(1, key="value")
@module.attribute
def decorated() -> None: ...


@functools.cache
async def decorated_async(): ...


def generic[T, U: int, *Ts, **P, V: (str, bytes) = str](x: T, *args: *Ts) -> list[T]:
    return [x]


def callable_params[**P, R](func: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:
    return func(*args, **kwargs)


def defaults(a=None, b=True, c=..., d=(1,), e=[], f={}, g=lambda x: x, h=-1):
    """The body is dropped."""
    return a
