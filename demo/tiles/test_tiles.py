from tiles import tile_count


def test_exact_fit():
    assert tile_count(512, 512, 256) == 4


def test_partial_tiles_count():
    assert tile_count(600, 300, 256) == 6


if __name__ == "__main__":
    for name, test in list(globals().items()):
        if name.startswith("test_"):
            test()
            print("ok", name)
