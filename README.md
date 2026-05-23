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

Модульный интерфейс: **только вкладки**, без меню. Layout и привязки данных — YAML в `public/config/`. Компоненты реализуются в коде и регистрируются в `src/components/register.ts`.

См. [config/README.md](./config/README.md) и [rusefui.md](./rusefui.md#архитектура-ui-реализовано).

## Структура

- `crates/rusefi-protocol` — CRC-пакеты, serial, handshake (`S`)
- `src-tauri` — Tauri-команды (`list_serial_ports`, `connect_ecu`, …)
- `public/config/` — вкладки и деревья компонентов (YAML)
- `src/core/` — реестр, загрузчик конфигов, data context
- `src/components/builtins/` — `connection`, `scalar-field`, `output-value`, layout-контейнеры
