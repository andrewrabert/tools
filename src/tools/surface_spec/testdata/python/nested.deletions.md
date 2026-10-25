# Surface Spec: Python
The set of `def` and `class` declarations the resulting combined diff
adds must equal the declarations under `## Additions`, and the set it
deletes must equal the declarations under `## Deletions`.

## Additions

No additions.

## Deletions

### nested.py
```python
def typed_only(values: Iterator[int]) -> int: ...


class TypedOnly:
    value: int


def in_elif(): ...


def in_else(): ...


def accelerated() -> bool: ...


def fallback() -> bool: ...


class Loaded: ...


def cleanup(): ...


def outer(x: int) -> int:
    def inner(y: int) -> int:
        def innermost() -> int: ...

    class Local:
        def method(self): ...


class Container:
    def method(self):
        def helper(): ...

        class InMethod:
            field: int

    class Nested:
        class Deeper:
            depth: int = 2


def in_for(): ...


def in_for_else(): ...


def in_while(): ...


def in_with(): ...


def in_match(): ...
```
