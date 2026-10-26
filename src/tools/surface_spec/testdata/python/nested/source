from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator

    def typed_only(values: Iterator[int]) -> int: ...

    class TypedOnly:
        value: int
elif OTHER:
    def in_elif(): ...
else:
    def in_else(): ...

try:
    import fast

    def accelerated() -> bool:
        return True
except ImportError:
    def fallback() -> bool:
        return False
else:
    class Loaded: ...
finally:
    def cleanup(): ...


def outer(x: int) -> int:
    def inner(y: int) -> int:
        def innermost() -> int:
            return 1

        return y + innermost()

    class Local:
        def method(self): ...

    return inner(x)


class Container:
    def method(self):
        def helper(): ...

        class InMethod:
            field: int

        return helper

    class Nested:
        class Deeper:
            depth: int = 2


for item in range(3):
    def in_for(): ...
else:
    def in_for_else(): ...

while False:
    def in_while(): ...

with open("f") as handle:
    def in_with(): ...

match command:
    case "go":
        def in_match(): ...
    case _:
        pass
