# Тестовые данные

## `rusefi_proteus_f7.ini`

Копия сгенерированного TunerStudio INI для платы **proteus F7** (из `rusefi/firmware/tunerstudio/generated/`).

Используется по умолчанию для:

- парсера `crates/rusefi-ini` (секция `[OutputChannels]`, `ochBlockSize`, `signature`);
- декодирования блока output channels в `OutputChannelsSource`.

Переопределение пути: переменная окружения `RUSEFI_INI_PATH`.

Обновление файла из дерева rusEFI:

```bash
cp /path/to/rusefi/firmware/tunerstudio/generated/rusefi_proteus_f7.ini test_data/
```
