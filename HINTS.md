# HINTS — подсказки для следующих разработок VRCraft-Horizon

> Практические приёмы, точки движка и порядок действий. Дополняет NOTES.md
> (там — «почему», здесь — «как/где»).

## Быстрый старт сессии

1. Прочитать: `NOTES.md` → спеку релиза (`docs/superpowers/specs/…`) →
   `mixins/vivecraft/horizon-mixin.toml` (текущие инъекции).
2. Порядок навыков: brainstorming → writing-plans → subagent-driven-development.
   Ledger — в `.superpowers/sdd/` (workspace на план, не смешивать).
3. Перед любым push: см. чек-лист в конце.

## Где что в Horizon (актуально для VR-инъекций)

| Что ищешь | Где |
|---|---|
| Frame loop, render, render_frame | `horizon/crates/horizon-client/src/game.rs` (`render` ~15531, `render_frame` ~15537) |
| Мировой рендер, native_world_drawn | `horizon/crates/horizon-client-render/src/gfx.rs` (world pass ~9500–11090, вызов native_world_drawn ~11046) |
| ViewSet / primary() | `horizon/crates/horizon-client-render/src/gfx/view.rs` (`views[0]` — читает весь движок) |
| WorldDrawn (device/queue/encoder/color/depth) | `horizon/crates/horizon-client-render/src/gfx/native_render.rs` |
| Титульный экран (меню, кнопки) | `horizon/crates/horizon-client-render/src/title.rs` + `title/` |
| UI-виджеты и регистрация контролов | `ui.rs`, `screen_draw.rs` — рисуют, но НЕ регистрируют: для кликабельности нужен регистрирующий helper (`ui::button`, `settings_shell::foot_button`) — см. Horizon CLAUDE.md «Controllers reach what registers» |
| Панорама/фон меню | `crates/horizon-client/src/game/screen_backgrounds.rs` |
| Интероп-паттерн wgpu↔Vulkan | `horizon/crates/horizon-hdr/src/lib.rs` (образец) |
| Дизайн-токены UI | `horizon/docs/design-system.md` (Horizon-фичи должны ему следовать) |
| i18n в игре | перевод-ключи (`Text::tr`, `lang::get`) — хардкод строк в UI запрещён |

## Приёмы инъекций

- **Приватные методы**: helper компилируется внутри целевого файла →
  приватное доступно. Так вызывается `game.render()` из HEAD-хелпера.
- **HEAD на render_frame** — единственный надёжный способ «сделать что-то
  до кадра»; AFTER_ASSIGN ломается на `match`-инициализаторах
  (E_UNSUPPORTED_SITE).
- **Гейт по флагу мода**: каждый хелпер начинает с
  `if !game.native_on(vivecraft_mod::ID) { return; }` — мод должен быть
  выключаем без следов.
- **State в runtime**: глобальное состояние — только через
  `with_runtime(|rt| …)` (Mutex-guard, краткие замыкания, НЕ держать
  между вызовами движка).
- **Новый XrStructureType**: сверить номер с официальными OpenXR headers;
  в нашем enum уже были коллизии. Новые pfn'ы — `opt_proc("xr…")`,
  обязательные — `load_pfn` при init.
- **Send/Sync для сырых XR-структур**: `unsafe impl` + комментарий про
  POD/Mutex-guard (образцы в xr.rs).

## Windows-специфика

- Стейджинг: сначала `rm -rf target/mixins/modded` (E_ATOMIC_STAGE на
  повторе).
- Компиляция staged ~4 мин — гонять в фоне, не блокировать сессию;
  результат `cargo check -p horizon-client-render` = главный гейт.
- Баш-сессия держит cwd между вызовами только внутри одного tool-вызова —
  используй абсолютные пути или `cd X && …` одной строкой.

## Чек-лист перед пушем релиза

1. `cargo test -p vivecraft-vr` — все зелёные (в 0.1.3: 16).
2. `tools/test_stage.py` — число инъекций совпадает с manifest.
3. Рестейджинг + staged `cargo check -p horizon-client-render` — exit 0.
4. `git status` — `horizon/` чист (движок не тронут), `.superpowers/`
   не коммитить (это workspace scratch).
5. Версия поднята в `horizon-mixin.toml` И `horizon-vr/Cargo.toml`
   (сейчас их держат в синхроне вручную).
6. Commit → `git push origin main`.
7. **`git tag v<версия> && git push origin v<версия>`** — ОБЯЗАТЕЛЬНО:
   лаунчер/реестр видят новую версию только через GitHub Release, а CI
   прикрепляет `.hnmod` к Release только на теге `v*`. Без тега лаунчер
   навсегда считает актуальной прошлую версию (0.1.3 чуть не потерялся
   из-за этого).
8. Обновить README (фичи) и при релизных изменениях NOTES.md.

## Роадмап-заметки

- **0.1.4** (спека готова: `docs/superpowers/specs/2026-11-10-vr-main-menu-design.md`):
  VR-меню + кнопка «VR: Вкл/Выкл», сессия без мира, blit меню в оба глаза,
  клик лучом. В меню blit один кадр в оба глаза — параллакс не нужен.
- **0.1.5**: полное 3D GUI в мире (инвентарь/чат/крафт как 3D-панели,
  как Java ViveCraft). Опираться на существующие GuiHandler/raycast
  (`gui.rs`) и опыт 0.1.4.
- Дальше (из Java ViveCraft, по приоритету игроков): VR-скелеты вместо
  quad-аватаров; полноценный vivecraft:data хаб; keyboard/Steam-клавиатура
  в меню; профильные пресеты FOV/разрешения на runtime.
- Legacy-бэкенды (Vive/Oculus legacy, NOVR) — НЕ переносить: OpenXR
  покрывает всё живое.
