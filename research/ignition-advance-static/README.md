# Статическая модель начальной карты УОЗ

Генерация стартовой таблицы углов опережения зажигания только по статическим параметрам двигателя.

## Запуск

```bash
cd research/ignition-advance-static

# все двигатели с эталоном
python generate_map.py --all

# один двигатель
python generate_map.py examples/turbo_6g72tt_3000gt.json

# сравнение модели со всеми эталонами
python compare_map.py

# один двигатель
python compare_map.py examples/turbo_4g63t_1g_dsm.json

# подбор коэффициентов по всем эталонам (усреднённый loss)
python fit_coefficients.py -o config/coefficients.json --refine --weighted
```

Опционально — свой файл коэффициентов:

```bash
python generate_map.py examples/turbo_6g72tt_3000gt.json -c config/coefficients.json
```

## Двигатели

Все карты на **единой сетке осей 3000GT**: RPM 600–8000 (16), MAP 15–320 kPa (17).

| Конфиг | Эталон | Источник |
|--------|--------|----------|
| `examples/na_k20_honda.json` | `reference/na_k20_honda.csv` | K20 Hondata |
| `examples/turbo_ej207_subaru.json` | `reference/turbo_ej207_subaru.csv` | EJ207 Link safe map |
| `examples/turbo_4g63_evo8_stock.json` | `reference/turbo_4g63_evo8_stock.csv` | Evo VIII stock ROM |
| `examples/turbo_6g72tt_3000gt.json` | `reference/turbo_6g72tt_3000gt.csv` | 3000GT TunerStudio |
| `examples/turbo_4g63t_1g_dsm.json` | `reference/turbo_4g63t_1g_dsm.csv` | 1G DSM TunerStudio |

## Вход (JSON)

- `engine` — геометрия, ГБЦ, топливо, тип наддува
- `axes.rpm` — ось оборотов, об/мин (16 точек, как в TunerStudio)
- `axes.map_kpa` — ось нагрузки / MAP, кПа (17 точек, как в TunerStudio: 15 … 320)

## Выход (CSV)

Матрица как в ECU-карте:

- **столбцы (слева направо)** — обороты, об/мин;
- **строки (снизу вверх на экране)** — нагрузка MAP, кПа (в файле сверху вниз — от большей к меньшей);
- **ячейки** — угол УОЗ, °.

При подозрительных значениях в stderr выводятся предупреждения.

## Коэффициенты

Все настраиваемые константы — в [`config/coefficients.json`](./config/coefficients.json).  
Поправка по MAP — линейные коэффициенты в `load_correction`:

- `vacuum.deg_per_10_kpa` — +° на каждые 10 kPa ниже `reference_map_kpa` (100 kPa);
- `boost.deg_per_0_1_bar` — ° на каждые 0.1 bar над атмосферой (напр. −1.2);
- `boost.aspiration_scale` — множитель для NA/turbo/supercharged.

## Структура

```
ignition-advance-static/
  config/coefficients.json   # коэффициенты модели
  reference/                 # эталонные CSV для подбора
  model/                     # Python-пакет
  examples/                  # примеры входных JSON
  generate_map.py            # генерация CSV
  compare_map.py             # модель vs эталон(ы)
  fit_coefficients.py        # подбор коэффициентов по всем эталонам
  resample_references.py    # привести эталоны к осям 3000GT
  import_msq_reference.py    # эталон из TunerStudio .msq
  output/                    # результаты (не в git)
```
