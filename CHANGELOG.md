# Changelog

## 0.4.5 — 2026-06-09

- **INI-кнопки** — `commandButton` из TunerStudio (Auto Calibrate ETB и др.) как `ini-command-button`, отправка `Z`-команд из INI.
- **Панели из INI** — генерация в `~/.rusEFI/projects/{project}/ui_panels/{ini_hash}/`; bundled YAML убран из репозитория; UI читает cache через Tauri.
- **Файл проекта** — расширение `.rusefui` (старые `.json` по-прежнему открываются).
- **INI / новый проект** — пустая signature ECU не блокирует загрузку кастомного INI.
