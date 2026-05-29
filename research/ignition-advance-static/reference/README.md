# Эталонные карты УОЗ

CSV в том же формате, что и `output/`. Имя файла совпадает со stem JSON из `examples/`.

## Оси

**Единая сетка (эталон — 3000GT):**

- RPM: 600, 1000, 1500, …, 8000 (16 точек)
- MAP: 15, 30, 45, …, 320 kPa (17 точек)

Карты из других ECU (K20, EJ207, Evo VIII) ресемплятся bilinear + clamp на краях. Исходные CSV — в `reference/raw/`.

```bash
python resample_references.py
```

## Сравнение

```bash
python compare_map.py
python compare_map.py examples/turbo_4g63t_1g_dsm.json
python compare_map.py --json
```

## Импорт из TunerStudio .msq

```bash
python import_msq_reference.py \
  ~/TunerStudioProjects/.../tune.msq \
  examples/turbo_6g72tt_3000gt.json \
  -o reference/turbo_6g72tt_3000gt.csv
```

Если в msq нет бина (например 80 kPa), строка интерполируется между соседними.
