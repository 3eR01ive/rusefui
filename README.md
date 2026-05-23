# rusefui

Кроссплатформенный интерфейс для rusEFI (Rust + Tauri + Vue), совместимый с бинарным протоколом TunerStudio.

Подробный план: [rusefui.md](./rusefui.md).

## Требования

- Rust (stable)
- Node.js 18+
- Linux: `webkit2gtk`, `libudev` (для serial)

## Разработка

```bash
npm install
npm run tauri dev
```

## Сборка

```bash
npm run build
npm run tauri build
```

## UI

**Vue** — отрисовка и действия пользователя (`dispatch`). **Rust** (`rusefui-runtime`) — логика сложных компонентов и (далее) подготовка данных по `bind.source`. Layout — YAML в `public/config/`.

См. [rusefui.md](./rusefui.md#архитектура-ui-реализовано) и [config/README.md](./config/README.md).

## Структура

- `crates/rusefi-protocol` — протокол ECU
- `crates/rusefi-ini` — парсер INI (output channels, тестовый `test_data/rusefi_proteus_f7.ini`)
- `crates/rusefui-runtime` — `EcuSession`, logic-компоненты, `OutputChannelsSource` (poll `O`)
- `test_data/` — фикстура INI для разработки (`RUSEFI_INI_PATH` для другого файла)
- `src-tauri` — `component_mount` / `component_dispatch`, событие `component-state`
- `src/composables/useRustComponent.ts` — подписка Vue на Rust state
- `public/config/` — YAML layout
- `src/components/builtins/` — SFC (presentation-only или тонкая обёртка)
