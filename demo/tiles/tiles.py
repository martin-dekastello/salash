"""Split a map into square tiles."""


def tile_count(width: int, height: int, size: int) -> int:
    """How many size x size tiles cover a width x height map."""
    cols = width // size
    rows = height // size
    return cols * rows


def tile_bounds(col: int, row: int, size: int) -> tuple[int, int, int, int]:
    """(left, top, right, bottom) of one tile, right/bottom exclusive."""
    return col * size, row * size, (col + 1) * size, (row + 1) * size
