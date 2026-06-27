import os

# Greeter module helper.
def greet(name: str) -> str:
    """Return a greeting for the given name."""
    return helper(name)


class Calculator:
    """A simple calculator."""

    def add(self, a: int, b: int) -> int:
        return a + b


def helper(name: str) -> str:
    return name.strip()
