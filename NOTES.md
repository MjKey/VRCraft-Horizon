# NOTES — архитектурные решения и находки VRCraft-Horizon

> Для будущих сессий/разработчиков. Здесь — НЕповторение README (фичи), а
> решения, подводные камни и «почему так», выстраданные в 0.1.0–0.1.3.

## Главный принцип порта

**Движок Horizon — только для чтения.** Чекаут закреплён на
`fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` (проверяется по
compatibility.json `source_sha256`). Весь код интеграции живёт в
`mixins/vivecraft/` и инъектируется через `horizon-mixin.toml`
(schema 6, rev 5a64af938ec86b00d53d16a408aabc0038fb3e57).
Если хочется «просто поправить один файл движка» — это ошибка: игроки
с GitHub не смогут собрать мод, патчи движка в `.hnmod` не попадают.

## Карта инъекций (26 в 0.1.3)

| Точка движка | Что делает мод |
|---|---|
| `horizon_client_render::gfx::View::ViewSet::single`-путь (`stereo_views`) | разворачивает один View в stereo-пару (len=2) |
| `horizon_client::game::Game::render_frame` (HEAD, `stereo_second_pass`) | правый глаз ДО левого: вызывает приватный `game.render()` с `stereo_eye=1` |
| `horizon_client_render::gfx::native_render::native_world_drawn` (`world_drawn`) | blit кадра в глаз → release → xrEndFrame; создаёт XR-сессию с Vulkan-binding |
| `Game` модуль `runtime.rs` (name `vivecraft_mod`, pub(crate)) | VrClientRuntime: on_frame/on_tick, настройки, инвентарь VR |
| `gfx.rs` модуль `vr_bridge.rs` (name `vr_bridge`, pub) | мост render-крейт ↔ vivecraft-vr; `stereo_views`, `on_world_drawn` |

- Helper-файлы компилируются **внутри целевого файла** движка → имеют
  доступ к приватным методам (поэтому `game.render()` из HEAD-хелпера
  работает) и обязаны соблюдать атрибуты файла (gfx.rs =
  `#![forbid(unsafe_code)]`!).
- Bundle-crate (`horizon-vr`) НЕ допускает `[target]`-таблицы в
  Cargo.toml (E_CRATE) — зависимости только plain.

## Стерео: почему HEAD на render_frame

- `AFTER_ASSIGN` требует простое `let x = call();` — на
  `let rendered = match self.render() {...}` даёт E_UNSUPPORTED_SITE.
- Схема: HEAD-хелпер рендерит ПРАВЫЙ глаз (eye=1, свап пары в
  `build_stereo_views_for_screen`, чтобы `views.primary()` = правый глаз),
  затем тело render_frame рендерит ЛЕВЫЙ. `world_drawn`-хук blit'ит каждый
  проход в свой глаз; левый проход (видя `stereo_right_done`) делает
  release + xrEndFrame. **Никакого чередования по кадрам** — оба глаза в
  одном xrEndFrame, как Java ViveCraft.

## OpenXR без C-биндингов

- `libloading` грузит `openxr_loader.dll`; все функции — через
  `xrGetInstanceProcAddr` + `transmute` в `unsafe extern "system"` typedef'ы.
  Структуры — свои `#[repr(C)]` с ручными `XrStructureType`-дискриминантами
  (калибруй по официальной таблице! в 0.1.3 была коллизия 17).
- Сырые свапчейн-изображения: `XrSwapchainImageVulkan2KHR` (type 1000029002)
  читаются как байты с шагом 64, VkImage-хендл по смещению 16.
- `XrInstanceExtensionProperties` = 17 конфликтует с
  `EventDataSessionStateChanged` (тоже 17)?? Нет: последнее — 17, первое
  переехало в enum как 2 ради уникальности (в OpenXR-заголовке
  InstanceExtensionProperties живёт в другом ряду). Внимание при добавлении
  новых тегов: **сверяйся с xr headers**, не нумеруй подряд.

## Vulkan interop (wgpu 30.0.1 + ash 0.38)

- Паттерн из `horizon/crates/horizon-hdr`: unsafe-интероп в отдельном
  крейте со своим `ash = "0.38"` (та же версия, что у wgpu-hal).
- Критичные пути типов (wgpu 30 прячет hal):
  - `wgpu::TextureUses` / `wgpu::TextureDimension` — реэкспорт **сверху**
    wgpu, НЕ `wgpu::hal::…`.
  - `wgpu::hal::TextureDescriptor` требует `memory_flags:
    wgpu::hal::MemoryFlags::empty()` и `view_formats: Vec::new()>` (не `&[]`).
  - `wgpu::hal::vulkan::vk` — приватный → свой ash.
- Хендлы: `ash::vk::Image::from_raw(u64)` (трейт `ash::vk::Handle`);
  `ash::Instance`/`ash::Device` (обёртки) — через `.handle()`.
- Доступ к внутренностям wgpu-Vulkan:
  `device.as_hal::<Vulkan>()` →
  `shared_instance().raw_instance()` (VkInstance),
  `raw_physical_device()`, `raw_device()`, `queue_family_index()`,
  `queue_index()` — всё методы **hal Device** (не Queue!).
- Graphics binding сессии: `XR_KHR_vulkan_enable2` включается при
  xrCreateInstance (запрос через `xrEnumerateInstanceExtensionProperties`),
  сессия создаётся ОТЛОЖЕННО (`create_session_with_vulkan`) — из render-хука,
  когда есть живой wgpu-Device. Без binding SteamVR не даёт swapchain'ы.
- Форматы свапчейнов: 37 = BGRA8_SRGB, 50 = RGBA8_SRGB (маппинг в
  `blit::eye_wgpu_format`); остальные → отказ от blit, desktop-рендер.

## Стейджинг и проверка

```sh
rm -rf target/mixins/modded          # ОБЯЗАТЕЛЬНО на Windows (E_ATOMIC_STAGE)
python tools/horizon.py stage --horizon ./horizon
cd target/mixins/modded && cargo check -p horizon-client-render -p horizon-client
```
- Инъекции есть и в render-крейте (vr_bridge), и в game.rs (runtime.rs) —
  staged check обязан компилировать ОБА, иначе лаунчер ловит ошибки,
  которые локальный гейт пропустил (так вышло в 0.1.3).
- `tools/test_stage.py` содержит assert на число инъекций — обновлять при
  добавлении.
- Полный цикл перед пушем: `cargo test -p vivecraft-vr` + staged check +
  `git status` (horizon/ должен быть чист).
- Helper компилируется в целевом файле движка: пути вида `crate::protocol`
  означают "модуль движка", а НЕ модуль vivecraft-vr — используй
  реэкспорты из `vr_bridge` (`vr::VivecraftPacket`), иначе E0433.
- В statics движка нельзя `HashMap::new()` (E0015) — только
  `LazyLock::new(|| …)`.

## Известные упрощения (намеренные)

- В меню кадр blit'ится ОДИН раз в оба глаза (0.1.4): в меню нет
  стерео-параллакса, второй полный render не нужен.
- VR-аватары — quads, не полный скелет.
- ViveItems — NBT на ванильных предметах (работа на ванильных серверах),
  не отдельные item-id.
- Server-side — headshot/стрелы через payload'ы; полного vivecraft:data-хаба
  как в Java-плагине нет.

## Куда смотреть при «ничего не работает»

1. `log::info!("[ViveCraft] …")` — XR init пишет причину фолбэка в
   Simulated (нет загрузчика? нет HMD? xrCreateSession упал?).
2. Мод требует **Vulkan** бэкенд (настройки графики Horizon). OpenGL →
   всегда Simulated/desktop.
3. Сессия без graphics binding = нет swapchain = нет картинки в шлеме
   (см. «Vulkan interop» выше).
