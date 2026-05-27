# Декларативные конфиги UI

Копия для разработки. **Runtime** читает файлы из `public/config/` (можно редактировать без пересборки Rust).

## Структура

| Путь | Назначение |
|------|------------|
| `app.yaml` | Список вкладок (`$tab: …`) |
| `tabs/*.tab.yaml` | Вкладка: заголовок + корень layout |
| `components/*.yaml` | Составной компонент (дерево `children`) |

## Правила

1. **`type:`** в YAML — SFC в `src/components/register.ts` (отрисовка).
2. **Logic в Rust** — только для сложных типов (`connection`, …), см. `crates/rusefui-runtime` и `src/core/rust-logic.ts`.
3. **`bind:`** — привязка к источнику данных (снимки готовит Rust). Имена полей/каналов **только в YAML**, не в SFC.
   - `source`: `config` | `outputChannels` | `textLog` | `knockScope` | `compositeLogger` | `connection` (и т.д. — **откуда данные**)
   - Тип logic-компонента (`dyno`, `simulation`) задаётся полем `type:`, не `bind.source`
   - `field` — одно поле; `fields` — список каналов; `params` — например `xBins`/`yBins`/`zBins`, `rpmField`/`tpsField`
3. **`$component: foo.bar`** — ссылка на файл `components/foo.bar.yaml`.
4. Вложенность: `children` у контейнеров (`stack`, `row`, `section`, `composite`) и у composite-файлов.

## Пример инстанса с edit-полем

```yaml
- type: scalar-field
  props:
    label: Hard RPM limit
  bind:
    source: config
    field: rpmHardLimit
```

## Пример output

```yaml
- type: output-value
  props:
    label: RPM
    unit: rpm
  bind:
    source: outputChannels
    field: RPMValue
```

## Пример графика (несколько каналов)

```yaml
- type: output-chart
  props:
    height: 240
    windowSeconds: 30
  bind:
    source: outputChannels
    fields:
      - RPMValue
      - coolant
```

## Пример таблицы config

```yaml
- type: config-table
  props:
    title: VE Table
  bind:
    source: config
    params:
      zBins: veTableTbl
      xBins: veRpmBins
      yBins: veLoadBins
```
