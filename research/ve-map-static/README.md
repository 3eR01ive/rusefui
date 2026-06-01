# Статическая модель стартовой VE-карты

Генерация начальной таблицы volumetric efficiency (VE) только по геометрии впуска и параметрам распредвала — без логов, wideband, MAF и обучаемых моделей.

## Модель

\[
\mathrm{VE}(\mathrm{rpm}, \mathrm{map}) = \mathrm{VE\_rpm}(\mathrm{rpm}) \times \mathrm{LoadFactor}(\mathrm{map}) \times \mathrm{BoostFactor}(\mathrm{map})
\]

| Шаг | Суть |
|-----|------|
| Flow Index | `ValveArea × cam_lift × cam_duration`, нормализация к `ReferenceFlowIndex = 350000` |
| VE_peak | `0.75 + 0.25 × clamp(FlowRatio, 0, 1)`, затем NA: 0.70…1.10, turbo: 0.70…1.20 |
| RPM_peak | `1500 + max(0, duration−220)×12 + (350 − runner_mm)×5`, затем **32–72%** от `max_rpm` (см. ниже) |
| Sigma | `1200 + (LSA − 106)×150`, clamp 800…3000 |
| VE_rpm | Гаусс вокруг RPM_peak + blend с полом `0.35 × VE_peak` |
| LoadFactor | NA: `(MAP/100)^0.65`, turbo: `(MAP/100)^0.85` |
| BoostFactor | Только turbo, MAP > 100 kPa: `1 + (MAP−100)/100 × 0.25` |

Внутри модели VE — доля **0.00 … 1.50**; в CSV ячейки — **проценты** (0.0 … 150.0, где 100.0 = 100% VE). Размер таблицы задаётся осями во входном JSON, не моделью.

Поля `displacement_cc` и `cylinders` зарезервированы для будущих расширений; в v1 не участвуют в формулах.

### Почему не `duration × 20` из исходного ТЗ

Advertised duration обычно **220–280°**. Формула `1500 + duration×20` для 260° даёт **6700 об/мин** ещё до учёта коллектора — пик гаусса уезжает к `0.95×max_rpm`, на WOT карта монотонно растёт к красной зоне (плюс clamp 150% по MAP). Физически пик наполнения чаще **середина диапазона** (примерно 35–65% от `max_rpm`); в модели учитывается только **прирост** длительности относительно базы 220°.

## Запуск

```bash
cd research/ve-map-static

python generate_map.py examples/na_2l_4cyl.json
python generate_map.py examples/turbo_2l_4cyl.json --diagnostics

# все примеры → output/
python generate_map.py --all
```

## Вход (JSON)

```json
{
  "engine": {
    "displacement_cc": 1998,
    "cylinders": 4,
    "max_rpm": 8000,
    "intake_runner_length_mm": 320,
    "intake_valve_diameter_mm": 34.0,
    "cam_duration_deg": 240,
    "cam_lift_mm": 10.2,
    "cam_lsa_deg": 110,
    "is_turbo": false
  },
  "axes": {
    "rpm": [500, 1000, ...],
    "map_kpa": [20, 30, ..., 250]
  }
}
```

Рекомендуемые диапазоны осей (вне модели): RPM **500 … max_rpm**, MAP **20 … 250 kPa**.

## Выход (CSV)

Формат как у ECU-карты (как в [ignition-advance-static](../ignition-advance-static/)):

- **столбцы** — RPM, об/мин;
- **строки** — MAP, кПа (в файле сверху вниз от большей к меньшей);
- **ячейки** — VE, % (один знак после запятой).

## Структура

```
ve-map-static/
  model/
    engine.py       # EngineParameters
    calculator.py   # VeMapCalculator
    io.py
  examples/         # входные JSON
  output/           # результаты (не в git)
  generate_map.py
  README.md
```

## Ограничения (ТЗ)

Запрещено в модели: AFR/lambda, wideband, MAF, логи, ML, таблицы известных моторов.

Цель: запуск двигателя, физическая правдоподобность, ошибка до ~15–20% относительно реального мотора — приоритет простоте и предсказуемости.
