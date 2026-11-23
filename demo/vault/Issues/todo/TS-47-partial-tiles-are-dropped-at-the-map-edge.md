---
id: "TS-47"
title: "Partial tiles are dropped at the map edge"
status: todo
source: local
created: 2026-11-20
moved: 2026-11-20T09:00:00
tags:
  - issue
---
# Partial tiles are dropped at the map edge

tile_count() floors the division, so a 600x300 map with 256px tiles loses its edge tiles. test_partial_tiles_count fails.
