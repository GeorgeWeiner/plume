"""Demo file: Python syntax highlighting showcase."""

from functools import lru_cache

GOLDEN_RATIO = 1.618033988749


@lru_cache(maxsize=None)
def fib(n: int) -> int:
    """Classic recursive fibonacci with memoization."""
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)


class Sequence:
    def __init__(self, limit=20):
        self.limit = limit
        self.values = [fib(i) for i in range(limit)]

    def ratios(self):
        # consecutive ratios converge to the golden ratio
        for a, b in zip(self.values[1:], self.values[2:]):
            if a:
                yield b / a


if __name__ == "__main__":
    seq = Sequence(limit=25)
    print("fib:", seq.values[:10])
    last = list(seq.ratios())[-1]
    print(f"ratio -> {last:.9f} (golden: {GOLDEN_RATIO})")
