# Декларативные конфиги UI

Копия для разработки. **Runtime** читает файлы из `public/config/` (можно редактировать без пересборки Rust).

## Структура

| Путь | Назначение |
|------|------------|
| `app.yaml` | Список вкладок (`$tab: …`) |
| `tabs/*.tab.yaml` | Вкладка: заголовок + корень layout |
| `components/*.yaml` | Составной компонент (дерево `children`) |

## Правила

1. **`type:`** в YAML — только типы, зарегистрированные в `src/components/register.ts` (реализованы в коде).
2. **`bind:`** — декларативная привязка к источнику: `connection`, `config`, `outputChannels`, `textLog`.
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
