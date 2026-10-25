# Surface Spec: Python
The set of `def` and `class` declarations the resulting combined diff
adds must equal the declarations under `## Additions`, and the set it
deletes must equal the declarations under `## Deletions`.

## Additions

### classes.py
```python
class Empty: ...


class Documented: ...


class Base(Parent, metaclass=Meta, flag=True):
    count: int
    name: str = "base"
    first = second = 0
    (x, y) = (1, 2)
    template = """first line\n  indented second line\n"""
    type Alias[T] = list[T]

    def method(self, value: int) -> int: ...

    @property
    def size(self) -> int: ...

    @size.setter
    def size(self, value: int) -> None: ...

    @staticmethod
    def make() -> "Base": ...

    @classmethod
    def build(cls, *args: object) -> "Base": ...

    async def run(self) -> None: ...

    class Inner(Generic[T]):
        value: T

        def get(self) -> T: ...

        class Innermost: ...


@dataclass(frozen=True)
class Point[T: float](*bases, **options):
    x: T
    y: T = 0


class Methods:
    def only(self): ...
```

## Deletions

No deletions.
