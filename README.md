# giga-throne

Красивый и быстрый десктопный клиент для VLESS, Hysteria2, TUIC, Trojan, Shadowsocks, WireGuard и прочего.
Нативный UI на Rust + [Slint](https://slint.dev), под капотом — [Throne](https://github.com/throneproj/Throne) в headless-режиме
(sing-box + xray-core).

Статус: ранний скелет.

## Сборка

```sh
cargo run -p giga-throne
```

Для подключений нужен бинарь Throne — см. [sidecar/README.md](sidecar/README.md).

## Лицензия

GPL-3.0-or-later, как и Throne. Спецификация протокола в `docs/throne-api/` — MIT, © The Throne Authors.

Шрифты (SIL OPEN FONT LICENSE 1.1, тексты в `crates/giga-throne/ui/fonts/*-OFL.txt`): Unbounded, Manrope, JetBrains Mono.
