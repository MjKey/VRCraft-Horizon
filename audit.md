# Технический аудит VRCraft-Horizon 0.1.5

**Дата:** 10 ноября 2026
**База:** Minecraft Horizon 26.3 (`fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca`)
**Оригинал для сравнения:** Java Vivecraft `Multiloader-26.3`
**Проверено:** полное чтение кода `mixins/vivecraft/` (~10.5k строк) + инъекции (~1.5k), сверка всех 25 хуков с движком, `cargo build --release` (exit 0), `cargo test -p vivecraft-vr` (16/16 зелёные), содержимое `target/vivecraft-0.1.5.hnmod`.

---

## Итоговый вердикт

**Архитектура рендера и трекеров — готова; интерактивность в шлеме — на уровне прототипа.**

В SteamVR (Vulkan-бэкенд, вход в мир, наличие лоадера — см. §1) мод **запустится в VR**: сессия создастся, позы HMD и контроллеров будут живые, оба глаза отрендерятся и уйдут в композитор. Но управление контроллерами **не работает** (§2.1), стрельба не следует за рукой (§2.5), FOV — десктопный, а не VR (§2.3). При невыполнении условий §1 — тихий откат в Simulated/flat.

Прежний аудит («100% готовности») был завышен и списывал логическую готовность за фактическую.

---

## 1. Запустится ли в SteamVR, или будет flat?

Будет VR только при выполнении **всех трёх** условий; иначе — Simulated (flat) с записью причины в лог `[ViveCraft] …`:

| # | Условие | Статус | Комментарий |
|---|---|---|---|
| 1 | `openxr_loader.dll` найден по PATH | ❌ **не выполнится у большинства игроков** | `.hnmod` лоадер не бандлит; на чистой Windows его в PATH нет. Java-версия проблемы не имела: LWJGL вёз `openvr_api.dll` внутри natives-jar'ов (`build.gradle:43-46`). **Положить dll в бандл можно только через `mixins/vivecraft/data/`** (читатель `.hnmod` `allowed()` в `native.rs:123` принимает только `.rs`/`.toml`/`data/`/текстовые файлы крейтов — двоичный файл вне `data/` отказывает весь бандл), и он **не оказывается рядом с игрой** (docs/native-mods.md: «both are compiled in; neither folder is beside the game when it runs»). Рабочие механизмы: (а) `[[post_build]]`-шаг, копирующий dll из `HORIZON_NATIVE_BUNDLE_DATA_DIR` в `HORIZON_NATIVE_DATA_DIR` (`native-data/vivecraft/` возле exe), затем в рантайме `native_built_data("vivecraft")` → `Library::new(абсолютный_путь)`; (б) `native_data!`/`include_bytes!` + распаковка в temp-файл при старте. |
| 2 | Бэкенд рендера — Vulkan | ⚠️ | `create_session_with_vulkan` + blit завязаны на `device.as_hal::<Vulkan>()`. OpenGL/DX12 → нет сессии → flat. В README для игрока не упомянуто. |
| 3 | Вход в мир | ⚠️ | `native_world_drawn` вызывается только на мировом рендере; в главном меню сессия не создаётся. VR-меню из спеки 0.1.4 не реализовано (v0.1.4-коммит — только bump версии). |

Релизная механика в порядке: `.hnmod` собран, тег `v0.1.5` запушен, лаунчер увидит релиз.

## 2. Критические дефекты (по убыванию важности)

### 2.1. Ввод контроллеров никогда не читается — главный баг
`xrGetActionStateBoolean/Float/Vector2f` загружены в `OpenXrProcs` (`xr.rs:1590-1592`), но **ни один вызов в коде не совершается**. `poll_events()` обновляет только *позы* контроллеров; поля `trigger`, `squeeze`, `thumbstick`, `button_primary/secondary/menu` навсегда остаются дефолтными (0/false). Следствия:
- нет клика по 3D-GUI / radial / клавиатуре / тач-хотбару (всё завязано на `trigger > 0.5`);
- нет smooth-локомоции стиком, snap/smooth turn (`thumbstick.x`), прыжка/спринта (squeeze);
- в Simulated-режиме то же: контроллеры движутся, кнопок нет вообще.

Мод в шлеме сейчас = «смотреть головой», всё остальное — с клавиатуры.

### 2.2. Bindings никогда не предлагаются
`xrSuggestInteractionProfileBindings` загружен, но не вызван ни разу. Рантайм не знает, как действия мапятся на `/user/hand/{left,right}`. Даже после починки 2.1 все action states будут `is_active=false`. Нужны профили: минимум `/interaction_profiles/khr/simple_controller`, целевые — valve/index, oculus/touch.

### 2.3. `xrLocateViews` никогда не вызывается
FOV глаз остаются дефолтными (±50°), а рендер-проекция строится из **экранного** FOV (`build_stereo_views_for_screen` берёт `base_projection`). В шлеме — letterboxed-копия десктопного кадра (blit с letterbox в `blit.rs:143-149`), не настоящая VR-перспектива.

### 2.4. `locate_space` с `time = 0`
Позы локируются с timestamp 0 вместо `frame_state.predicted_display_time` (`xr.rs:1888,1926`). На строгих рантаймах — stale/invalid-позы или validation-ошибки.

### 2.5. Стрельба из лука не следует за рукой (AimFix не применён)
`on_arrow_loose` подменяет только силу натяжения (`runtime.rs:1015`); направление стрелы — из ванильной камеры (куда смотрит голова). `on_entity_shoot_bow` — пустая заглушка (`let _ = (player, projectile)`, `runtime.rs:950`), хотя серверные AimFix-данные для неё и хранятся (`server_aim_for_player`). Для VR это фича №1 после ходьбы — она не работает.

### 2.6. Blit без layout-transition
`texture_from_raw` + `copy_texture_to_texture` в предположении, что acquired-изображение уже в copy-compatible layout; OpenXR-спека отдаёт swapchain-изображения в `UNDEFINED` layout. Возможны Vulkan validation-ошибки / чёрные кадры на части рантаймов. Нужен явный `vkCmdPipelineBarrier` (ash) перед копированием.

### 2.7. Мелкое, но реальное
- `recenter_yaw_offset` вычисляется, но **нигде не потребляется** (только сеттер/геттер): F8 фактически сбрасывает лишь `rotation_radians = 0`.
- Телепорт исполняется через `tp @s` — на vanilla-сервере без прав оператора не сработает (network-путь `server_allows_teleport` есть, локальный fallback — команда).
- `XrSessionState::Visible` не обрабатывается как рабочий (только `Focused || Synchronized`): при потере фокуса окна позы замораживаются в simulated-ветку.

## 3. Что перенесено (главное)

| Подсистема | Статус | Где |
|---|---|---|
| OpenXR 1.0 динамический лоадер (аналог MCVR/MCOpenVR) | ✅ | `xr.rs` (2528 строк): полный C-ABI вручную, deferred-сессия с `XR_KHR_vulkan_enable2` Vulkan binding |
| Стерео: оба глаза в одном кадре → xrEndFrame | ✅ | HEAD на `render_frame`: правый глаз → левый → submit (`stereo_second_pass.rs`, `vr_bridge.rs`) |
| Reversed-Z, асимметричный FOV | ✅ математика | `stereo.rs`, покрыта юнит-тестами |
| 6DoF камера без лимита 10° roll | ✅ | `camera_pose` + `camera_shake` хуки |
| `vivecraft:data` протокол, wire-совместимый | ✅ | `protocol.rs` (1029 строк), roundtrip-тест, версии -1..=10 |
| 10 трекеров локомоции | ✅ | `locomotion.rs`: teleport/sneak/crawl/jump/climb/swim/run/row/vehicle/horse |
| Ближний бой свингом >2.5 м/с, DDA voxel raycast | ✅ | `interaction.rs` |
| Двуручный лук (натяжение) | ⚠️ частично | натяжение есть, направление — нет (§2.5) |
| Roomscale коллизии, step-up 0.1–1.0 м | ✅ | `resolve_roomscale_voxel_collision` |
| Еда/питьё у рта, телескоп, рюкзак, камера, хаптика | ✅ | `interaction.rs` |
| 3D GUI, radial, клавиатура, тач-хотбар, wrist HUD | ✅ | `gui.rs` |
| FBT (OSC-трекеры) | ✅ | `osc.rs` |
| Сервер: relay + headshot 3.0x | ⚠️ частично | данные AimFix хранятся, не применяются |
| 3D-аватары удалённых VR-игроков | ✅ (quad, заявлено) | `collect_world_primitives` |
| Настройки VRSettings, persist, 14 кнопок UI | ✅ | `settings.rs` |

Интеграция с движком чистая: все 25 хуков из `horizon-mixin.toml` существуют в движке; `horizon/` не тронут (git clean); принцип «engine read-only» соблюдён.

## 4. Что НЕ перенесено (главное)

1. **Ввод контроллеров** (§2.1–2.2) — критично.
2. **VR-главное меню** (спека есть, кода нет) — до входа в мир шлем не получает картинку игры.
3. **Настоящий VR-FOV/проекция из рантайма** (§2.3).
4. **AimFix применённый** (направление стрел/атак из руки, §2.5).
5. Полный vivecraft:data-хаб на сервере (только relay + headshots; заявлено как упрощение).
6. VR-скелеты игроков (quad-аватары; заявлено).
7. **Comfort-vignette и lens stencil** — функции написаны, но **не подключены к рендеру** (не вызываются нигде).
8. Legacy-бэкенды (Vive/Oculus legacy, NOVR) — осознанно не портированы, это правильно (OpenXR покрывает всё живое).

## 5. Приоритет исправлений

1. **Лоадер в бандле через `data/` + post_build** (или temp-распаковка) + README: «требуется Vulkan-бэкенд». Без этого всё остальное бессмысленно для игроков.
2. **Чтение action states + suggested bindings** (профили khr/simple, valve/index, oculus/touch) — без этого нет VR-управления.
3. **AimFix в `on_entity_shoot_bow`** — направление стрел из руки.
4. `xrLocateViews` → реальные FOV/eye-позы в проекции и submission-слоях.
5. `predicted_display_time` в `locate_space`.
6. VR-меню (спека 0.1.4 уже написана).
7. Layout-transition в blit; потребление `recenter_yaw_offset`; обработка `Visible`-состояния; подключение vignette-шейдера.

## 6. Верификация

- `cargo build --release -p vivecraft-vr` — **exit 0** (6m28s).
- `cargo test -p vivecraft-vr` — **16 passed / 0 failed**.
- Тесты покрывают математику (стерео, проекции), трекеры, протокол, GUI-raycast — но **не могут поймать §2.1–2.3**, т.к. hardware-путь подменяется дефолтными позами. Это и есть причина, почему «всё зелёное» при нерабочем вводе.
- Все 25 инъекций сверены с API движка — расхождений нет.
