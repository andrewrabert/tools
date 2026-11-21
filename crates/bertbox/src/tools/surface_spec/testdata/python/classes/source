class Empty:
    pass


class Documented:
    """Only a docstring."""


class Base(Parent, metaclass=Meta, flag=True):
    """The docstring is dropped."""

    count: int
    name: str = "base"
    first = second = 0
    x, y = 1, 2
    template = """first line
  indented second line
"""
    type Alias[T] = list[T]
    import os
    ...
    print("dropped")
    if count:
        hidden = 1

    def method(self, value: int) -> int:
        return value

    @property
    def size(self) -> int:
        return 0

    @size.setter
    def size(self, value: int) -> None: ...

    @staticmethod
    def make() -> "Base":
        return Base()

    @classmethod
    def build(cls, *args: object) -> "Base": ...

    async def run(self) -> None: ...

    class Inner(Generic[T]):
        value: T

        def get(self) -> T: ...

        class Innermost:
            pass


@dataclass(frozen=True)
class Point[T: float](*bases, **options):
    x: T
    y: T = 0


class Methods:
    def only(self): ...
