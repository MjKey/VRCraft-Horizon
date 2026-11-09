# VRCraft-Horizon 0.1.4 — VR-главное меню (спека)

> История: сессия порта ViveCraft→Horizon. Готовая база 0.1.3 запушена в
> https://github.com/MjKey/VRCraft-Horizon (commit f27c8bd): одновременное
> стерео, OpenXR + `XrGraphicsBindingVulkan2KHR`, SteamVR-совместимость,
> 26 инъекций, staged-компиляция зелёная. Эту спеку исполняет НОВАЯ сессия.

## Цель

VR работает **с главного меню**, как в Java ViveCraft: игрок запускает игру
в шлеме, видит меню (панорама + кнопки) в стерео, кликает кнопки лучом
контроллера, и игра входит в мир уже в VR. Плюс **кнопка «VR: Вкл/Выкл»**
прямо в главном меню.

Версия мода: 0.1.4. Полное 3D GUI (инвентарь/чат/крафт в 3D-панелях) —
отложено в 0.1.5, НЕ входит в этот релиз.

## Жёсткие ограничения (унаследованные)

- **Движок Horizon не редактировать** — чекаут закреплён на
  `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca`, проверяется compatibility.json.
  Вся интеграция — только через `horizon-mixin.toml` инъекции
  (HEAD/RETURN/OVERWRITE/AFTER_CALL и т.д.), bundle-crate `vivecraft-vr`
  и helper-файлы в `mixins/vivecraft/`.
- `horizon-client-render` (gfx.rs и всё в нём) — `#![forbid(unsafe_code)]`:
  unsafe только в `vivecraft-vr` (паттерн: `blit.rs`, свой `ash = "0.8"` нет,
  `ash 0.38` как у wgpu-hal).
- Bundle-crate НЕ допускает `[target]`-таблицы в Cargo.toml (E_CRATE).
- Стейджинг: `rm -rf target/mixins/modded && python tools/horizon.py stage
  --horizon ./horizon`, затем `cargo check -p horizon-client-render` из
  `target/mixins/modded`. Перед стейджингом на Windows обязательно
  удалять modded-папку (E_ATOMIC_STAGE).
- Никаких чередований по кадрам — только одновременное стерео.

## Архитектура 0.1.3, на которую опираемся

- `stereo_second_pass` — HEAD-инъекция на `Game::render_frame`
  (`mixins/vivecraft/stereo_second_pass.rs`): перед основным (левым) проходом
  вызывает `game.render()` с `stereo_eye=1`; `build_stereo_views_for_screen`
  свапает пару, когда `stereo_eye==1`, так что `views.primary()` несёт правый
  глаз.
- `world_drawn`-хук (`on_world_drawn` в vr_bridge.rs) — blit кадра в глаз
  через `vivecraft_vr::blit::blit_frame_to_eye_safe`, затем
  release+`xrEndFrame`. Он же сейчас создаёт XR-сессию
  (`create_session_with_vulkan` из `vulkan_handles(world.device, …)`).
- `begin_frame` вызывается из `VrClientRuntime::on_frame` (runtime.rs,
  инъекция на кадр) — уже работает и в меню (гейтится только наличием сессии).
- XR-курсор GUI уже существует: `rt.gui.cursor_on_gui` + uv → координаты
  экрана (используется в on_frame, runtime.rs:335–341).

## Что построить

### 1. Кнопка «VR: Вкл/Выкл» в главном меню

- Инъекция в отрисовку титульного экрана (`crates/horizon-client-render/src/
  title.rs`, найти точку где рисуются кнопки «Играть/Настройки/Выход»;
  для контроллеров Horizon требует регистрирующий helper — см. CLAUDE.md
  Horizon «Controllers reach what registers» — проверь, какой путь
  регистрации доступен из инъекции; если полноценная кнопка не
  инъектируется, допустим fallback: пункт в existing меню настроек
  Horizon, куда мод уже инъектируется).
- Текст — перевод-ключ, не хардкод («vivecraft.vr_toggle»), стиль —
  по дизайн-токенам Horizon (docs/design-system.md).
- Переключает `rt.settings.vr_enabled` (сохранение — через существующий
  механизм настроек мода).

### 2. XR-сессия без мира

- Сейчас сессия создаётся только из `world_drawn` (не вызывается в меню).
  Перенести/продублировать вызов `create_session_with_vulkan` в кадровой
  инъекции (`on_frame` / `stereo_second_pass` helper), доставая device из
  рендерера игры (найти: как из `Game` достать `wgpu::Device` до входа в мир
  — вероятно через `native_render` globals или gfx Renderer; исследовать).
- В меню глазам нужны позы: если HMD-трекинг уже идёт (сессия+view space) —
  реальные позы; иначе фиксированная поза «сидя, взгляд вперёд», IPD из
  настроек. Составление projection-слоя уже есть (`end_frame`).

### 3. Blit кадра меню в глаза

- В меню `render_frame` выполняется, но `world_drawn` нет. Нужен хук
  «кадр (меню) готов» — варианты: (а) инъекция на конец `render_frame`
  (RETURN/третий helper) с blit итогового фреймбуфера; (б) существующий
  screen/UI-хук мода (в mod-модуле уже есть screen_mouse/screen_char —
  посмотреть, какой рендер-путь мода доступен). Выбрать при исследовании;
  критерий: есть доступ к итоговой цветовой текстуре кадра и энкодеру.
- Правый глаз в меню: НЕ нужен второй полный `game.render()` (в меню
  стерео-параллакс почти не важен) — ДВОЙНОЙ blit одного кадра в оба глаза
  допустим и дешевле; но если infra позволяет reuse stereo pass — ок.
  Решение за исполнителем по простоте. Это не «чередование»: оба глаза
  получают кадр в одном xrEndFrame.

### 4. Клик по кнопкам меню лучом

- VR-курсор уже маппит луч → uv → экранные координаты (runtime.rs:337–341).
- В меню этот путь должен работать: убедиться, что `cursor_on_gui`
  активен и без мира, и что координаты скармливаются тому же обработчику
  кликов, что и мышь (screen_mouse-инъекция уже есть — расширить её
  на меню-экран).

### 5. Панорама/фон в VR

- Фон меню Horizon (панорама) рисуется движком как есть — blit всего кадра
  уже несёт её. Дополнительно можно наклонять камеру меню от HMD-поворота
  (как ViveCraft) — nice-to-have, не блокер.

## Критерии приёмки

- Игра, запущенная с активным SteamVR и Vulkan-бэкендом: меню видно в шлеме
  (обе глаза, не перевёрнуто), кнопки кликабельны лучом, вход в мир не
  сбрасывает VR.
- Кнопка «VR: Вкл/Выкл» переключает и состояние сохраняется между
  запусками.
- Без шлема/OpenGL — поведение 0.1.3 без регрессий (desktop-меню как было).
- `cargo test -p vivecraft-vr` зелёные; staged `cargo check
  -p horizon-client-render` зелёный; чекаут horizon чист.
- Коммит + push на main после приёмки. Версия 0.1.4 в
  horizon-mixin.toml и Cargo.toml.

## Порядок работы (новая сессия)

1. Прочитать эту спеку + `mixins/vivecraft/horizon-mixin.toml` + vr_bridge.rs
   + blit.rs + stereo_second_pass.rs + runtime.rs (on_frame).
2. superpowers:brainstorming → уточнение точек инъекции (исследование
   рендер-пути меню) → superpowers:writing-plans →
   superpowers:subagent-driven-development (как в 0.1.3).
3. Опираться на ledger-паттерн `.superpowers/sdd/progress.md` (новый
   workspace для нового плана).
