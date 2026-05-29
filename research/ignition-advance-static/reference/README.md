# Эталонные карты УОЗ

CSV в том же формате, что и `output/`. Имя файла совпадает со stem JSON из `examples/`:

| Эталон | Конфиг |
|--------|--------|
| `turbo_6g72tt_3000gt.csv` | `examples/turbo_6g72tt_3000gt.json` |
| `turbo_4g63t_1g_dsm.csv` | `examples/turbo_4g63t_1g_dsm.json` |

## Откуда брать

1. Экспорт / копирование таблицы **Ignition Table** из TunerStudio.
2. Или импорт из `.msq` — см. `import_msq_reference.py`.

## Сравнение

```bash
# все двигатели с эталоном + сводка
python compare_map.py

# один двигатель
python compare_map.py examples/turbo_4g63t_1g_dsm.json

# уже сгенерированный output vs эталон
python compare_map.py examples/turbo_4g63t_1g_dsm.json --candidate output/turbo_4g63t_1g_dsm.csv

# метрики JSON
python compare_map.py --json
```

Подбор коэффициентов: `fit_coefficients.py` усредняет ошибку по всем парам example↔reference.

## Импорт из TunerStudio .msq

```bash
python import_msq_reference.py \
  ~/TunerStudioProjects/3000gt-backup/3000gt-release_2024-10-23_15.43.14.msq \
  examples/turbo_6g72tt_3000gt.json \
  -o reference/turbo_6g72tt_3000gt.csv
```

Если в JSON есть бины, которых нет в msq (например 80 kPa), строка **интерполируется** между соседними.
