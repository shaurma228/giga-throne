# giga-throne

Десктопный VPN/proxy-клиент (VLESS, Hysteria2, TUIC, Trojan, SS, WireGuard…) с красивым UI и упором на скорость.
Это **только морда**: всю протокольную логику, парсинг ссылок, подписки, генерацию конфигов sing-box/xray, TUN,
системный прокси и kill switch делает [Throne](https://github.com/throneproj/Throne), запущенный в headless-режиме.
Мы с ним общаемся по Throne API.

Язык общения с пользователем — русский. Код, идентификаторы и комментарии — на английском, тексты UI — на русском.

## Как работаем

- **Код пишут субагенты на haiku** (`Agent` с `model: "haiku"`). Основной агент ставит задачу, даёт контекст
  (файлы, спеку, критерии готовности) и проверяет результат: `cargo build`, `cargo test`, `cargo clippy`, ревью диффа.
  Сам основной агент пишет только документацию, планы и мелкие правки по итогам ревью.
- Можно подключать **Antigravity CLI**: `agy -p "<задача>" --model gemini-3.8-flash-medium` (есть `-high`, `-low`).
  **Перед каждым использованием** проверить лимиты: `agy -p "/usage"`. Если в нужной строке (Gemini Models,
  Weekly или Five Hour) осталось меньше ~20% — agy не трогать, писать через haiku.
- Задачи для субагентов — маленькие и проверяемые: один модуль или один экран, с явным «готово, когда…».
- **Коммитит основной агент сам**, без напоминаний, после каждой проверенной единицы работы (build, test, clippy, fmt зелёные).
  Одна идея — один коммит: фикс отдельно от фичи, рефакторинг отдельно. Если в одном файле смешаны разные идеи,
  разносить по коммитам частичным staging (`git hash-object -w` + `git update-index --cacheinfo`; интерактивный `add -p` недоступен).
  Каждый коммит должен собираться.
- Сообщение коммита: на английском, `scope: what changed` (scope — `throne-api`, `app`, `ui`, `sidecar`, `engine`,
  `sysproxy`, `docs`…), в теле — зачем и что важно знать. В конце — трейлер `Co-Authored-By` из системных инструкций сессии.
- Никогда не коммитить ключи, `api.key`, бинарь Throne и прочее из `sidecar/`. Не пушить, не делать `--amend`/rebase
  опубликованной истории и не трогать чужие коммиты без просьбы пользователя.

## Архитектура

```
giga-throne (Rust, один бинарь)
  ├─ UI на Slint (.slint)          экраны, компоненты, тема
  ├─ app: состояние, tokio         связка UI ↔ API, опрос статуса и статистики
  ├─ throne-api                    клиент Throne API: шифрованный TCP-протокол + типизированные запросы
  └─ sidecar                       запуск, перезапуск и остановка headless Throne
        │  TCP 127.0.0.1:<port>, ключ из нашего api.key
        ▼
Throne --headless  →  ThroneCore (Go: sing-box + xray-core)
```

Почему так: в Throne парсинг ссылок и сборка конфигов — ~20k строк C++ под 25+ протоколов. Переписывать это
бессмысленно, а headless-режим уже отдаёт весь движок через API, и фиксы протоколов приходят из апстрима бесплатно.

### Структура репо

```
crates/
  throne-api/     библиотека: протокол (handshake, кадры), клиент, типы запросов/ответов
  giga-throne/    бинарь: main, app-состояние, sidecar, платформа (трей, deeplinks)
    ui/           .slint: app.slint (корень), theme.slint (токены), components/, screens/
docs/
  throne-api/     спека Throne API (MIT, из throneproj/Throne-api; коммит в UPSTREAM_COMMIT)
  design/         макеты экранов (.dc.html) — источник правды для вёрстки
sidecar/          бинарь Throne для разработки (в git не попадает, см. sidecar/README.md)
```

## Throne API — главное

Полная спека: `docs/throne-api/protocol.md` (транспорт) и `docs/throne-api/api.md` (запросы). Читать перед
любой работой с `throne-api`.

- Это **не HTTP**. TCP, обе стороны сразу шлют hello: сервер 16 байт `S`, клиент `0x01 || C` (17 байт).
- Ключи сессии: `k_c2s = HMAC-SHA256(K, "throne-api/1 c2s" || S || C)`, `k_s2c` аналогично с `s2c`.
- Кадр: `len` (u32 BE) `|| ChaCha20-Poly1305(msg)`; nonce = `0x00000000 || counter_u64_be`, aad = те же 4 байта `len`.
  Счётчик свой на каждое направление, начинается с 0.
- Лимиты `len`: от 16; клиент→сервер ≤ 1 MiB, сервер→клиент ≤ 64 MiB. Проверять **до** чтения тела.
- Сообщения — JSON: запрос `{"method","path","query"?,"body"?}`, ответ `{"status","body"}` или `{"status","error":{"code","message"}}`.
- Строго запрос → ответ по очереди на одном соединении. Сервер рвёт соединение после 60 с тишины — переподключаться прозрачно.
- Ключ — 32 байта, текстом base64url без паддинга (43 символа).
- Тест-векторы `docs/throne-api/vectors/v1.json` **обязаны** проходить в тестах `throne-api`.
- Крейты для крипты: `chacha20poly1305`, `hmac`, `sha2`, `rand`/`getrandom`, `base64`. Свою криптографию не писать.

### Sidecar

Запускаем `Throne --headless --data-dir <data>/throne --api-listen 127.0.0.1:<port> --api-key-file <data>/api.key`.
Ключ генерируем сами при первом старте (права 0600). Путь к бинарю: `GIGA_THRONE_SIDECAR`, иначе `sidecar/Throne/Throne`,
иначе рядом с нашим исполняемым файлом. Порт по умолчанию 9096, чтобы не конфликтовать с обычным Throne на 9095.
Коды выхода и watchdog описаны в `api.md` (раздел Headless Throne).
Ещё передаём `--restart-mode exit`: перезапуск (код 75) делает наш супервизор. Нужен Throne **≥ 1.4.0**: в 1.3.x headless-режима нет.
Проверить, что он жив: `sidecar/Throne/Throne --headless --health --data-dir <data>/throne`.

**Нужен Throne из нашего форка.** Стоковый Throne не умеет управлять маршрутами через API, поэтому эндпоинты
`/v1/routes*` и `/v1/rule-sets` (спека: `docs/throne-api/routes-proposal.md`) живут в форке `~/git/Throne`, ветка
`feat/route-api` (от тега 1.4.0-beta.4, PR в апстрим планируется). Собирается только Qt-часть, Go не нужен:
```sh
cd ~/git/Throne && mkdir -p build && curl -fLso build/srslist.h \
  https://raw.githubusercontent.com/throneproj/routeprofiles/rule-set/srslist.h
cmake -S . -B build -GNinja -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DINPUT_VERSION="1.4.0-beta.4+route-api.$(git rev-parse --short HEAD)" && ninja -C build Throne
cp build/Throne ~/git/giga-throne/sidecar/Throne/Throne
```
`ThroneCore` в `sidecar/Throne/` — из релиза 1.4.0-beta.4 (совместим, пока форк на том же теге); стоковый бинарь лежит
рядом как `Throne.release`. Без `INPUT_VERSION` бинарь сообщает пустую версию (`Status.version == ""`). Что именно стоит — в `sidecar/VERSION`. Без форка приложение работает, но экран маршрутов
покажет, что движку не хватает API. Тесты с настоящим движком: `cargo test -p giga-throne -- --ignored`.

## Дизайн

Макеты — `docs/design/*.dc.html`, а также канвас https://claude.ai/artifact/3iHon3YRVvfWGXT93sxRBQ.
Экраны: Главная (большая кнопка подключения), Серверы (группы, карточки), Импорт (ссылка/подписка/QR/файл), Маршруты
(пресеты по регионам + свои), Редактор маршрута (режимы «Просто» и «Продвинуто»), UI-кит.

Токены (в коде живут в `ui/theme.slint`, хардкодить цвета в экранах нельзя):

| Токен | Значение | | Токен | Значение |
|---|---|---|---|---|
| bg | `#0E0E12` | | text | `#F4F4F6` |
| surface | `#17171D` | | text-2 | `#B4B4BE` |
| panel | `#121217` | | muted | `#8A8A96` |
| raised | `#22222B` | | accent (по умолч.) | `#C8FF4D` |
| line | `#2A2A34` | | on-accent | `#0E0E12` |
| ping ok / mid / bad | `#9BE564` / `#FFB547` / `#FF6B6B` | | accent-варианты | `#FF7AD9`, `#5CE1FF`, `#FF9F43` |

Протоколы (тег: текст / фон 12%): VLESS `#9DB8FF`, HY2 `#FFB0E6`, TUIC `#8FE3F2`, TROJAN `#FFC98A`, SS `#C9B8FF`, WG `#A8F0C0`.

Шрифты: Unbounded (заголовки), Manrope (интерфейс), JetBrains Mono (цифры). Радиусы: 10–16 px у элементов, 20–24 у окон и модалок.
Окно без системной рамки, свой заголовок. Минимальный размер 1040×680.

Правила UI:
- Только то, что Slint рендерит быстро: заливки, градиенты, тени, скругления, анимации свойств. Без backdrop blur и «стекла».
- Анимации короткие: 150–300 мс, `ease-out`. Ничего не крутится без причины.
- UI-поток никогда не ждёт сеть: все запросы в tokio, результат в UI через `slint::invoke_from_event_loop` / `Weak::upgrade_in_event_loop`.
- Текст ≥ 12 px, кликабельные цели ≥ 36–44 px, контраст текста ≥ 4.5:1.
- Ловушки Slint 1.18: фиксированный по высоте ребёнок `HorizontalLayout` прижимается к верху строки — центрировать через
  `VerticalLayout { alignment: center; … }`. `Flickable` без явной ширины передаёт родителям ограниченный `max-width`
  своего содержимого и ужимает всю колонку — оборачивать в `Rectangle` и задавать `width/height: parent.*`.
- Вёрстку проверять глазами, а не только компиляцией: `GIGA_THRONE_PAGE=… ./target/debug/giga-throne &`, через ~7 с
  `spectacle -b -n -a -o <scratchpad>/shot.png` (только наше окно), затем `pkill -INT -f 'target/debug/giga-throne$'`
  (штатный выход: снимает системный прокси и останавливает движок) и смотреть PNG. Субагентам давать этот цикл.

## Команды

```sh
cargo build                       # сборка воркспейса
cargo run -p giga-throne          # запуск
cargo test --workspace            # тесты (включая тест-векторы протокола)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
GIGA_THRONE_PAGE=2 cargo run -p giga-throne        # только debug: старт на странице 0 Главная, 1 Маршруты, 2 новый маршрут, 2:<id> маршрут <id>
GIGA_THRONE_EDITOR_MODE=advanced cargo run -p giga-throne  # только debug: редактор стартует в «Продвинуто»
```

Тулчейн задан в `rust-toolchain.toml` (stable). Версии зависимостей добавлять через `cargo add`, не вписывать руками.

## Соглашения

- Лицензия GPL-3.0-or-later (как у Throne). Новый файл из чужого кода — с указанием источника и лицензии.
- Ошибки: `thiserror` в библиотеке, `anyhow` в бинаре. Никаких `unwrap()` вне тестов, кроме доказуемо невозможных случаев с комментарием.
- Логи — `tracing`. Ключ API никогда не логировать и не показывать в UI целиком.
- Новый протокол или настройка движка — сначала проверить, есть ли это в `api.md`. Если нет — мы этого не делаем, а не пишем в обход.
