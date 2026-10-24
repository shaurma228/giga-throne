# sidecar

Сюда кладётся бинарь Throne (headless) для локальной разработки — содержимое папки в git не попадает.

Скачай релиз **1.4.0 или новее** (в 1.3.x нет headless-режима) с https://github.com/throneproj/Throne/releases и распакуй сюда так, чтобы был `sidecar/Throne/Throne`
(на Windows `Throne.exe`), либо укажи путь через `GIGA_THRONE_SIDECAR=/path/to/Throne`.

Для маршрутов нужен бинарь `Throne` из нашего форка (см. раздел Sidecar в CLAUDE.md): положи его вместо `Throne`,
а `ThroneCore` оставь из релиза 1.4.0-beta.4 (linux-amd64, sha256 архива `51016e9b…2ebb48`).

Запиши, что лежит, в `sidecar/VERSION`.
