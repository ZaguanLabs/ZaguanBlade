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


def load_config():
    """Read the database URL from the environment (M4.2 reads_env fixture)."""
    return os.environ["DATABASE_URL"]


def getenv(name):
    """User-defined getenv shadow (M4.2 anchoring): a bare `getenv(...)` call is
    NOT `os.getenv`, so calling it must NOT emit a reads_env edge."""
    return name


def read_shadowed():
    """NEGATIVE reads_env fixture: calls the user-defined `getenv` above, so no
    reads_env edge is emitted (only the usual call edge to `getenv`)."""
    return getenv("NOT_ENV")
