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

## Структура

- `crates/rusefi-protocol` — CRC-пакеты, serial, handshake (`S` / `Q`)
- `src-tauri` — Tauri-команды (`list_serial_ports`, `connect_ecu`, …)
- `src/views/ConnectionPage.vue` — страница подключения
