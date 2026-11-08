# Исчерпывающий технический план полного переноса ViveCraft (Java) на Minecraft Horizon (Rust)

Данный документ представляет собой детальный архитектурный и пошаговый инженерный план для достижения **100% функционального паритета** между оригинальным Java-модом **Vivecraft** (`Multiloader-26.3`, ~588 классов) и портом **VRCraft-Horizon** на нативном движке **Minecraft Horizon** (`26.3`).

План устраняет все блокеры, закрывает все архитектурные пробелы, охватывает физику перемещений, коллизии roomscale, полет на элитрах, пространственный 3D-звук, серверный AimFix, хэдшоты, кастомные предметы и полный стек OpenXR.

---

## Архитектурная диаграмма полной системы

```mermaid
flowchart TD
    subgraph Hardware ["VR Оборудование и Ввод"]
        HMD["Шлем OpenXR (Meta Quest, Index, Vive, Pico)"]
        CTRL0["Правый контроллер (Main Hand)"]
        CTRL1["Левый контроллер (Offhand)"]
        FBT["OSC Трекеры FBT (ноги, таз, грудь) :9000"]
    end

    subgraph CoreVR ["Крейт vivecraft-vr (Rust Core Engine)"]
        XR["XrContext (xr.rs) - OpenXR 1.0 + Actions + Haptics"]
        OSC["OscTrackerReceiver (osc.rs) - UDP FBT"]
        VRDATA["VrData / Pose Matrices (vr_data.rs)"]
        STEREO["Stereo Projections (stereo.rs) - Asymmetric Reversed-Z"]
        LOCO["Locomotion 10 Trackers (locomotion.rs) + Roomscale Physics"]
        INTERACT["Combat, Bow, Shield, Elytra, Tools (interaction.rs)"]
        GUI["3D GUI, Radial Menu, 3D Keyboard (gui.rs)"]
        AUDIO_VR["Spatial Audio HRTF Orientation (audio_vr.rs)"]
        NET["VivecraftNetworkManager (protocol.rs) - 32 пакета"]
    end

    subgraph MixinBridge ["Мост миксина (mixins/vivecraft)"]
        RUNTIME["runtime.rs (Связующий контроллер)"]
        VRBRIDGE["vr_bridge.rs"]
        STEREOVIEWS["stereo_views.rs (ViewSet Overwrite)"]
        INJECTIONS["horizon-mixin.toml (Инъекции Horizon Native Hooks)"]
    end

    subgraph HorizonClient ["Движок Horizon Client & Server (crates/)"]
        GAME["horizon-client (Game loop, Player, Input, World)"]
        RENDER["horizon-client-render (WGPU GFX, ViewSet, Shaders)"]
        AUDIO_SYS["horizon-client-render::audio (Mixer, Listener)"]
        PRIMITIVES["WorldPrimitives (Линии, Квады, Текст в 3D)"]
        PROTO["horizon-protocol (CustomPayload 'vivecraft:data')"]
        SERVER["horizon-server (AimFixHandler, Arrow Headshots, Hitbox)"]
    end

    HMD --> XR
    CTRL0 --> XR
    CTRL1 --> XR
    FBT --> OSC
    OSC --> XR

    XR --> VRDATA
    VRDATA --> LOCO
    VRDATA --> INTERACT
    VRDATA --> GUI
    VRDATA --> STEREO
    VRDATA --> AUDIO_VR
    VRDATA --> NET

    STEREO --> VRBRIDGE
    VRBRIDGE --> STEREOVIEWS
    STEREOVIEWS -->|Два глаза Reversed-Z| RENDER

    LOCO --> RUNTIME
    INTERACT --> RUNTIME
    GUI --> RUNTIME
    AUDIO_VR --> RUNTIME
    NET --> RUNTIME

    RUNTIME -->|native_frame, native_tick| GAME
    RUNTIME -->|native_movement_input, native_push_player| GAME
    RUNTIME -->|native_camera_pose (6DoF HMD)| GAME
    RUNTIME -->|native_render_hand_pose| GAME
    RUNTIME -->|set_listener (HMD 3D Position & Forward/Up)| AUDIO_SYS
    RUNTIME -->|native_world_primitives (Лазер, Дуга, HUD, Клавиатура)| PRIMITIVES
    RUNTIME -->|CustomPayload Client/Server| PROTO
    RUNTIME -->|AimFix, Headshot, Hitbox Override| SERVER
```

---

## Этап 1. Полноценный OpenXR 1.0 Runtime и ввод (`xr.rs`, `osc.rs`)

> [!CRITICAL]
> **Главный блокер рантайма:** Текущий `XrContext::try_init_openxr` создает лишь `XrInstance` и `XrSystemId`. Без `XrSession` и цикла `xrPollEvent`/`xrSyncActions` при включенном SteamVR/Meta Quest Link игра переходит в режим `hardware_connected == true`, отключает симуляцию поз, а шлем застывает в координатах `(0,0,0)`, блокируя камеру персонажа.

### 1.1. Динамическая таблица процедур OpenXR 1.0 (`xrGetInstanceProcAddr`)
- [ ] Расширить `XrContext` полным набором указателей на функции:
  - **Сессия и пространства:** `xrCreateSession`, `xrDestroySession`, `xrBeginSession`, `xrEndSession`, `xrRequestExitSession`, `xrCreateReferenceSpace`, `xrDestroySpace`, `xrLocateSpace`, `xrLocateViews`, `xrPollEvent`.
  - **Свопчейн и кадры:** `xrEnumerateViewConfigurationViews`, `xrEnumerateSwapchainFormats`, `xrCreateSwapchain`, `xrDestroySwapchain`, `xrEnumerateSwapchainImages`, `xrAcquireSwapchainImage`, `xrWaitSwapchainImage`, `xrReleaseSwapchainImage`, `xrWaitFrame`, `xrBeginFrame`, `xrEndFrame`.
  - **Система действий (OpenXR Actions & Haptics):** `xrStringToPath`, `xrCreateActionSet`, `xrCreateAction`, `xrSuggestInteractionProfileBindings`, `xrAttachSessionActionSets`, `xrCreateActionSpace`, `xrSyncActions`, `xrGetActionStateBoolean`, `xrGetActionStateFloat`, `xrGetActionStateVector2f`, `xrGetActionStatePose`, `xrApplyHapticFeedback`, `xrStopHapticFeedback`.

### 1.2. Графический биндинг и управление состоянием сессии
- [ ] Реализовать поддержку графического биндинга для бэкенда WGPU (`XrGraphicsBindingVulkanKHR` для Vulkan или `XrGraphicsBindingD3D12KHR` для DirectX 12).
- [ ] Реализовать конечный автомат `XrSessionState` (`IDLE` $\to$ `READY` $\to$ `SYNCHRONIZED` $\to$ `VISIBLE` $\to$ `FOCUSED` $\to$ `STOPPING` $\to$ `LOSS_PENDING`).
- [ ] Обрабатывать `xrPollEvent` каждый кадр; корректно вызывать `xrBeginSession` при переходе в `READY` и `xrEndSession` при выходе.

### 1.3. Референсные пространства и опрос 6DoF поз
- [ ] Создать референсные пространства:
  - `XR_REFERENCE_SPACE_TYPE_STAGE` — базовое калиброванное комнатное пространство (roomscale, пол на $Y=0$);
  - `XR_REFERENCE_SPACE_TYPE_LOCAL` — сидячее пространство (seated);
  - `XR_REFERENCE_SPACE_TYPE_VIEW` — пространство взгляда HMD.
- [ ] Опрашивать позы глаз шлема через `xrLocateViews` в пространстве `STAGE`: сохранять левый и правый `eye_positions`, `eye_rotations` и асимметричные углы обзора `eye_fovs` (`XrFovf: angle_left, angle_right, angle_up, angle_down`).
- [ ] Настроить Action Set `/actions/gameplay` и привязать профили:
  - `/interaction_profiles/oculus/touch_controller`
  - `/interaction_profiles/valve/index_controller`
  - `/interaction_profiles/htc/vive_controller`
  - `/interaction_profiles/khr/simple_controller`
- [ ] Опрашивать позы `grip` и `aim` для обеих рук через `xrGetActionStatePose` и `xrLocateSpace`.
- [ ] Считывать аналоговые значения стиков, курков и нажатия кнопок (`trigger`, `squeeze`, `thumbstick`, `A/X`, `B/Y`, `menu`).
- [ ] **Защитный фоллбэк:** Если OpenXR не перешел в `FOCUSED` или позы невалидны (`location_flags & XR_SPACE_LOCATION_POSITION_VALID_BIT == 0`), продолжать симуляцию 6DoF поз через мышь/клавиатуру, исключая замерзание камеры.

### 1.4. Тактильный отклик (Haptic Feedback)
- [ ] В `XrContext::on_frame_end` выгружать очередь `pending_haptics.drain(..)`.
- [ ] Передавать структуру `XrHapticVibration` в `xrApplyHapticFeedback`:
  - `duration = duration_micros * 1000` нс;
  - `frequency = frequency_hz`;
  - `amplitude = amplitude.clamp(0.0, 1.0)`.

### 1.5. Сетевой OSC-модуль для трекинга тела (Full-Body Tracking)
- [ ] Реализовать `mixins/vivecraft/horizon-vr/src/osc.rs`:
  - Неблокирующий сокет UDP на порту `127.0.0.1:9000` (`OSCTrackerReceiver.java`).
  - Парсинг стандартных пакетов VRChat OSC:
    - `/tracking/trackers/{1..7}/position` $(x, y, z)$;
    - `/tracking/trackers/{1..7}/rotation` $(pitch, yaw, roll)$.
  - Передача поз стоп, коленей, таза и груди в `XrContext::set_osc_tracker_pose`.

---

## Этап 2. Стереоскопический рендеринг и шейдерный конвейер (`stereo.rs`, `stereo_views.rs`, `gfx.rs`)

> [!IMPORTANT]
> **Главный блокер рендера:** В `horizon-client-render` (`Renderer::render` в `gfx.rs`) рендерер обрабатывает только `views.primary()` (индекс 0 — левый глаз). Правый глаз шлема (`views.views()[1]`) не участвует в проходе рендера мира, вызывая монокулярную картинку в одном глазу шлема.

### 2.1. Истинные асимметричные Reversed-Z проекции OpenXR
- [ ] В `build_stereo_views_for_screen` (`stereo.rs`, `lib.rs`) генерировать матрицы через функцию `openxr_perspective_reversed_z_rh`:
  ```rust
  let (left_proj, left_aspect, left_tan_fov) =
      openxr_perspective_reversed_z_rh(raw_frame.eye_fovs[0], NEAR_PLANE, far_plane);
  let (right_proj, right_aspect, right_tan_fov) =
      openxr_perspective_reversed_z_rh(raw_frame.eye_fovs[1], NEAR_PLANE, far_plane);
  ```
- [ ] Формировать матрицы вида `left_view` и `right_view` со строгим учетом физических смещений зрачков (`eye_positions`) и ориентаций (`eye_rotations`).

### 2.2. Двуглазый проход геометрии в `horizon-client-render`
- [ ] В `crates/horizon-client-render/src/gfx.rs` адаптировать проход мира `r_world_pass`:
  - Если `views.views().len() == 2`, выполнять проход рендера геометрии чанков, сущностей и партиклов для обоих глаз;
  - Для шлема OpenXR — выводить текстуры каждого глаза в соответствующие свопчейны `XrSwapchainImage`;
  - Для монитора ПК — отображать настраиваемый Mirror View (левый глаз, правый глаз, Side-by-Side или стабилизированное сглаженное окно).

### 2.3. Снятие ограничения крена головы (`roll`)
- [ ] В стандартном коде Horizon `native_camera_shake` обрезает `shake.roll` константой `SHAKE_MAX_DEGREES = 10.0`.
- [ ] Полную 6DoF ориентацию головы игрока (`Quat` ориентации HMD с произвольным креном в 360°) передавать напрямую в матрицу `View.view` в `stereo_views.rs`, минуя ограничения shake-системы.

### 2.4. Маска скрытых областей линз (Stencil Mesh) и виньетка комфорта
- [ ] Реализовать построение трафаретной сетки невидимых углов линз (`xrGetVisibilityMaskKHR` / `StencilHelper.java`) для отсечения пикселей до выполнения тяжелых фрагментных шейдеров (экономия до 15-20% GPU).
- [ ] Добавить затемняющую виньетку комфорта (Comfort Vignette) при резком плавном перемещении и поворотах для снижения укачивания.

---

## Этап 3. Полная локомоция, физика Roomscale и коллизии (`locomotion.rs`, `runtime.rs`, `VRPlayer`)

> [!WARNING]
> **Текущий статус:** Алгоритмы части трекеров описаны в `locomotion.rs`, однако физические векторы импульсов и смещений игнорируются в `runtime.rs`, а перемещение внутри игровой зоны комнаты проходит сквозь стены без учета воксельной геометрии мира.

### 3.1. Передача физических импульсов и телепортации в движок
- [ ] В `runtime.rs::on_tick`:
  ```rust
  let push = movement_out.roomscale_world_delta + movement_out.velocity_impulse;
  if push.length_squared() > 1.0e-6 {
      game.native_push_player([f64::from(push.x), f64::from(push.y), f64::from(push.z)]);
  }
  ```
- [ ] При наличии `movement_out.teleport_target`:
  - Проверять валидность поверхности (твердый блок, свободное пространство в 2 блока для тела);
  - Вызывать локальное перемещение в движке Horizon (`native_player_teleport`);
  - Отправлять сетевой пакет `TeleportPayloadC2S`.

### 3.2. Физика перемещения по комнате и предотвращение прохождения сквозь стены (`VRPlayer::doPlayerMoveInRoom`)
- [ ] Полноценный порт механики `VRPlayer.java:453-570`:
  - Вычислять физическое смещение головы `newHeadPivot` относительно `roomOrigin`;
  - Если активен FBT, использовать среднюю точку стоп трекеров для базового положения ног;
  - Формировать актуальный AABB хитбокс игрока в целевой точке $(x, y, z)$;
  - Проверять коллизию с вокселями через `game.level().noCollision(bb)`:
    - **При отсутствии коллизии:** сдвигать игрока `player.setPosRaw(x, y, z)` без смещения `roomOrigin`;
    - **При столкновении с блоком:** проверять возможность автоподъема на ступеньку/полукарниз (шаги по 0.1м вверх до 1.0м высоты). Если найдена свободная позиция — поднимать игрока, воспроизводить звук шага `stepSound` и корректировать `roomOrigin.y`;
    - **При непреодолимой преграде (стена):** физически блокировать продвижение персонажа в стене (pushback эффект), предотвращая читерский просмотр сквозь стены.

### 3.3. Приседания и ползание (`SneakTracker`, `CrawlTracker`)
- [ ] **Реалистичное приседание:** При опускании HMD ниже `sneakThreshold` (по умолчанию высота игрока минус 0.25м) активировать `sneak = true`, сужая хитбокс персонажа до 1.5м.
- [ ] **Реалистичное ползание:** При опускании HMD ниже `crawlThreshold` (0.75м от пола):
  - Переводить персонажа в `Pose.SWIMMING` (ванильная поза ползания высотой 0.6м);
  - Отправлять пакет `CrawlPayloadC2S(true)`.

### 3.4. Активация специализированных трекеров локомоции
- [ ] **Лестницы и скалолазание (`ClimbTracker`):**
  - Проверять блоки лестниц (`minecraft:ladder`), лиан (`minecraft:vine`), подмостков (`scaffolding`), светящихся лиан;
  - При наличии `Climbey Claws` разрешать хват за любые твердые блоки (`ClimbeyBlockmode`);
  - Реализовать захват ступени курком/хватом, физическое подтягивание тела и возможность выпрыгивания вверх рывком рук.
- [ ] **Плавание брассом (`SwimTracker`):**
  - Отслеживать погружение в воду/лаву (`in_water`);
  - Вычислять сведение и разведение рук от лица назад, генерируя импульс движения брассом;
  - Автоматически активировать анимацию спринт-плавания при интенсивных взмахах.
- [ ] **Гребля на лодке (`RowTracker`):**
  - При нахождении в лодке отслеживать погружение и гребные движения левого и правого весла в воде;
  - Преобразовывать движения в поступательную скорость лодки и крутящий момент поворота (`yaw_delta_deg`).
- [ ] **Лошади и верховая езда (`HorseTracker`):**
  - Расчет виртуальных поводьев между шеей лошади и руками контроллеров;
  - Натяжение левого/правого повода для плавного поворота;
  - Резкий взмах рукой вниз («пришпоривание/хлыст») для кратковременного ускорения лошади.
- [ ] **Транспорт и авто-спешивание (`VehicleTracker`):**
  - При физическом отходе игрока ногами в комнате на расстояние $>1.0$м от вагонетки/лодки/лошади вызывать автоматическое спешивание (`sneak = true`).
- [ ] **Прыжковые ботинки (`JumpTracker`):**
  - Поддерживать резкий толчок руками вниз при прыжке в экипированных Jump Boots.

### 3.5. Хоткеи управления
- [ ] Добавить в `runtime.rs`:
  - `KEY_RECENTER` (`F8`): мгновенный сброс угла румскейла по направлению взгляда и центрирование сидячего режима;
  - `KEY_TELEPORT` (`T`): переключение/триггер параболической телепортации.

---

## Этап 4. Боевая система, стрельба, физические взаимодействия и Elytra (`interaction.rs`, `runtime.rs`)

### 4.1. Двуручная стрельба из лука и арбалета (`BowTracker`)
- [ ] Определять лук/арбалет в любой руке (`holding_bow`, `holding_crossbow`, поддержка левшей).
- [ ] Захват тетивы второй рукой по нажатию триггера контроллера в радиусе 0.15м от тетивы.
- [ ] Натяжение тетивы с расчетом дистанции натяжения и нарастающей виброотдачей в руку.
- [ ] Выстрел в момент отпускания тетивы: расчет силы выстрела, сброс стрелы, применение AimFix траектории.
- [ ] Подключить хуки `native_arrow_nock` и `native_arrow_loose` в Horizon.

### 4.2. Физический ближний бой и добыча ресурсов (`SwingTracker`)
- [ ] Фиксация скорости движения инструмента/оружия: удар засчитывается только при линейной скорости кончика оружия $>2.5$ м/с (защита от читерского потряхивания контроллером).
- [ ] Рейкастинг вокселей из позиции острия оружия на дистанцию `reach * world_scale`.
- [ ] При попадании по мобу: инициировать удар `game.native_client_attack` с виброоткликом и спавном веерных партиклов размашистого удара (sweep attack) в плоскости взмаха.
- [ ] При попадании по блоку: инициировать удар по блоку (`native_left_click_block`).

### 4.3. Физическое блокирование щитом (Roomscale Shield Blocking)
- [ ] Порт `ServerPlayerMixin.java:158-200`:
  - Если в любой руке находится щит, проверять его пространственную ориентацию в момент прилета стрелы или атаки моба;
  - Если угол щита перекрывает направление атаки — блокировать урон и воспроизводить звон отбития щитом даже без зажатия кнопки мыши/приседания.

### 4.4. Физическое поедание пищи и питье зелий (`EatingTracker`)
- [ ] **Исправление формулы рта:**
  ```rust
  let mouth_pos = hmd_pos + room_pre.hmd.get_custom_vector(Vec3::new(0.0, -0.12, -0.05));
  ```
- [ ] При поднесении съедобного предмета ко рту на дистанцию $<0.14$м:
  - Автоматически инициировать использование предмета (`native_use_item_on`);
  - Воспроизводить периодический хруст поедания и тактильную вибрацию каждые 4 тика.

### 4.5. Полет на надкрыльях в VR (VR Elytra Flight)
- [ ] Порт механики полета элитр:
  - Режим планирования с расправленными в стороны руками («поза самолета» / T-pose);
  - Управление тангажом (вверх/вниз) и креном (влево/вправо) через наклон вектора линии между кистями рук относительно торса;
  - Взмахи обеими руками вниз в полете дают дополнительный аэродинамический толчок вверх;
  - Использование фейерверков: ракета вылетает по физическому направлению контроллера, держащего ракету.

### 4.6. Подзорная труба у глаза (`TelescopeTracker`)
- [ ] Инъекция в `native_zoom` / `native_compute_fov`:
  - При поднесении трубы к любому глазу шлема активировать узкий угол обзора с плавным оптическим приближением `factor = 1.0 / zoom_factor`.

### 4.7. Рюкзак за спиной (`BackpackTracker`) и сенсорный пояс хотбара
- [ ] Завод руки за плечо/спину:
  - За спиной основной рукой: переключение оружия (извлечение меча/инструмента за спиной);
  - За спиной второй рукой: быстрая смена щита/факела в левой руке (`native_player_swap_hands`).
- [ ] Сенсорный наручный/поясной хотбар (`InteractiveHotbarModule`): прикосновение кончиком контроллера к слоту на виртуальном поясе мгновенно выбирает слот (`native_set_selected_slot`).

### 4.8. Ручная VR-камера (`CameraTracker`) и скриншоты
- [ ] Возможность брать в руки виртуальную фото/видео камеру, наводить видоискатель на сцену и делать скриншоты нажатием курка точно из ракурса объектива камеры.

### 4.9. Интерактивные двери, люки и удочка (`DoorBlockVRMixin`, `FishingHookVRMixin`)
- [ ] Тактильный отклик при закрытии/открытии дверей, ворот и люков в радиусе 5 блоков (`triggerHapticPulse(0, 250)`).
- [ ] Удочка:
  - Визуальный старт лески точно из кончика трехмерного удилища в руке (`FishingHookRendererVRMixin`);
  - Тактильная вибрация контроллера при клеве: слабая пульсация при подергивании поплавка и мощный импульс при поклевке рыбы.

### 4.10. Поведение эндерменов (Enderman Gaze & Freeze)
- [ ] Расширение конуса взгляда с 1.4° до 5.7° (`EndermanMixin`), облегчающее провокацию/заморозку эндермена взглядом в VR.
- [ ] Эндермен поворачивает голову и смотрит строго на физическое положение HMD игрока в 3D мире (`EndermanFreezeWhenLookedAtMixin`).

---

## Этап 5. Пространственный 3D Звук и HRTF ориентация (`audio_vr.rs`, `horizon-client-render::audio`)

> [!NOTE]
> В оригинальном моде `SoundEngineVRMixin.java` ориентация и позиция звукового слушателя (OpenAL Listener) привязываются к координатам центрального глаза HMD и его векторам `forward` и `up`. В Horizon аудиомикшер по умолчанию берет координаты плоской камеры.

### 5.1. Привязка позиции слушателя к физическому HMD
- [ ] В `runtime.rs` при каждом вызове обновления камеры передавать в микшер Horizon (`audio.set_listener`) мировую позицию центрального глаза VR-шлема (`eye_center_world`).

### 5.2. Полноценная 6DoF ориентация слушателя с учетом крена (Roll)
- [ ] В `crates/horizon-client-render/src/audio.rs` функция `basis(yaw, pitch)` рассчитывает базис без учета угла крена головы.
- [ ] Добавить в аудиомикшер расчет базиса слушателя на основе полного кватерниона вращения HMD:
  ```rust
  let forward = hmd_quat * Vec3::NEG_Z;
  let up = hmd_quat * Vec3::Y;
  let right = hmd_quat * Vec3::X;
  ```
- [ ] Это обеспечивает идеальное позиционирование звуков в пространстве при наклонах головы набок.

### 5.3. Направленная виброотдача при получении урона
- [ ] При получении урона игроком считывать вектор атаки из пакета `DamageDirectionPayloadS2C` и подавать тактильный импульс в контроллер с соответствующей стороны (слева/справа).

---

## Этап 6. Пространственный 3D GUI, Радиальное меню, 3D Клавиатура и Меню-миры (`gui.rs`, `world_primitives.rs`)

### 6.1. Летающий 3D экран и лазерный указатель
- [ ] При открытом интерфейсе (инвентарь, меню паузы, сундуки) отображать экран как полигональный квад в 3D мире перед глазами игрока.
- [ ] Отрисовывать лазерный луч из правого контроллера с яркой точкой на экране.
- [ ] Рассчитывать пересечение луча с квадом экрана через `GuiHandler::get_tex_coords_for_cursor` и преобразовывать UV координаты в экранные координаты Horizon.
- [ ] Эмулировать перемещение мыши и клики триггером контроллера (`native_screen_mouse_button_pressed_pre`, `native_screen_mouse_dragged_pre`).

### 6.2. Радиальное меню быстрого доступа (Radial Menu)
- [ ] Вызов радиального колеса нажатием выделенной кнопки на контроллере.
- [ ] Отрисовка 8 секторов вокруг руки через `WorldPrimitives`:
  - 1: Инвентарь
  - 2: Смена перспективы (1st / 3rd person)
  - 3: Выбросить предмет в руке (Drop)
  - 4: Чат
  - 5: Виртуальная клавиатура
  - 6: Ручная камера
  - 7: Калибровка высоты пола
  - 8: Переключение Seated / Standing
- [ ] Выбор сектора наклоном аналогового стика с тактильным щелчком при переходе между секторами.

### 6.3. Виртуальная 3D-клавиатура (Keyboard Handler)
- [ ] Отображение клавиатуры в пространстве с раскладками (EN, RU, символы, цифры).
- [ ] Два режима ввода:
  - Лазерный указатель с кликом триггером;
  - Физические барабанные палочки на контроллерах для набора текста физическими ударами по клавишам (`PhysicalKeyboard::update_physical_touch`).
- [ ] Передача введенных символов в активное текстовое поле (`native_screen_char_typed_pre`).

### 6.4. Наручный HUD (Wrist HUD)
- [ ] Отрисовка полосок здоровья, брони, сытости, кислорода и статус-эффектов на левом запястье игрока.
- [ ] Автоматическая подсветка HUD при повороте запястья экраном к глазам.

### 6.5. 3D Меню-миры в главном меню (Menu Worlds)
- [ ] Порт `MenuWorldRenderer.java`:
  - Инъекция в `native_draw_menu_background`;
  - Вместо статичной плоской панорамы загружать и рендерить интерактивную 3D-диораму с покачивающимися деревьями, водой и мобами в реальном времени вокруг игрока.

---

## Этап 7. Мультиплеерный сетевой протокол, серверный AimFix и хэдшоты (`protocol.rs`, `AimFixHandler`)

### 7.1. Полный сетевой протокол `vivecraft:data` (32 пакета)
- [ ] Обеспечить сериализацию и десериализацию всех пакетов C2S и S2C:
  - `Version` (0), `VRMode` (1), `Teleport` (2), `WorldScale` (3), `Draw` (4), `Setting` (5), `ActiveHand` (6), `Crawl` (7), `Climb` (8), `Height` (9), `DamageDirection` (10), `AimDirOverride` (22), `AimPosOverride` (23) и пакеты данных поз FBT.
- [ ] Отправка позы `VivePlayer` на сервер каждый сетевой тик (20 Гц).

### 7.2. Серверный перехватчик `AimFixHandler`
- [ ] Порт `AimFixHandler.java`:
  - Встраивание в пайплайн обработки сетевых пакетов клиента;
  - При получении пакетов `ServerboundUseItemPacket`, `ServerboundUseItemOnPacket`, `ServerboundPlayerActionPacket`:
    - Временно подменять координаты игрока `(x, y, z)` и углы `yaw`/`pitch` на реальные координаты и направление контроллера прицеливания (`aimPos`, `aimDir`);
    - Вызывать ванильный обработчик пакета (спавн стрелы, бросок зелья/снежка, постановка блока происходят точно из физического положения руки контроллера);
    - Мгновенно восстанавливать исходную позицию игрока после обработки пакета.

### 7.3. Серверные хэдшоты из лука (`AbstractArrowMixin`)
- [ ] Проверка попадания стрелы в область головы сущности (`hitPos.y > entity.getEyeY() - 0.25`).
- [ ] При хэдшоте умножать базовый урон на `BOW_STANDING_HEADSHOT_MULTIPLIER` (множитель 3.0x).
- [ ] Спавнить крит-партиклы `ParticleTypes.CRIT` и проигрывать характерный звон попадания `SoundEvents.ITEM_BREAK`.

### 7.4. Динамическая высота хитбокса игрока на сервере
- [ ] На основе высоты HMD динамически пересчитывать `eyeHeight` и AABB игрока на сервере:
  - Стоя: 1.8м;
  - Присев: 1.5м;
  - Ползком / вплавь: 0.6м.
- [ ] Предотвращение ложных срабатываний античита на "moved wrongly" при комнатном хождении и телепортации.

### 7.5. Отображение 3D аватаров других VR-игроков в мультиплеере
- [ ] Инъекция в `native_player_submit`:
  - Считывание сетевых поз других VR-игроков из `network.remote_players`;
  - Настройка трансформаций костей модели: наклон и поворот головы по HMD, поворот груди/пояса, позиционирование рук по контроллерам, анимация взмахов надкрылий элитр синхронно с руками.

---

## Этап 8. Кастомные предметы, крафты и теги (`ViveItems`, `ViveItemTags`, `recipes`)

### 8.1. Регистрация предметов Vivecraft
- [ ] Зарегистрировать через хук `native_items`:
  - `Climb Claws` (`vivecraft:climbey_claw` / неразрушимые ножницы с тегом `vivecraft.item.climbclaws`);
  - `Jump Boots` (`vivecraft:jump_boots` / неразрушимые кожаные ботинки зеленого слизистого цвета с тегом `vivecraft.item.jumpboots`).
  - Пасхалки Алисы в Стране Чудес: пирог роста «EAT ME» и зелье уменьшения «DRINK ME» (меняют масштаб игрока `worldScale`).

### 8.2. Датапак рецептов крафта
- [ ] Создать рецепты в `data/vivecraft/recipe/`:
  - `climbclaws.json`: Паучий глаз + Ножницы $\to$ Когти для лазания;
  - `jumpboots.json`: Кожаные ботинки + Блок слизи $\to$ Прыжковые ботинки.

### 8.3. Реестры тегов предметов и блоков
- [ ] `ViveItemTags`: теги для распознавания стрел, кисточек, компасов, арбалетов, удочек, щитов, копий, мечей, подзорных труб.
- [ ] `ViveBlockTags`: теги `climbable` для блоков, по которым разрешено карабкаться.

---

## Этап 9. Полная карта инъекций `horizon-mixin.toml`

Для работы всего функционала таблица инъекций моста `horizon-mixin.toml` расширяется до полного набора хуков движка Horizon:

| ID инъекции | Точка входа в Horizon (`target`) | Назначение |
| :--- | :--- | :--- |
| `stereo_views` | `ViewSet::single` | Переопределение на Reversed-Z стереопару для обоих глаз |
| `frame` | `Game::native_frame` | Опрос OpenXR кадров, обновление поз и тактильных очередей |
| `tick` | `Game::native_tick` | Тик локомоции, 10 трекеров, позы тела и отправка пакетов |
| `camera_pose` | `Game::native_camera_pose` | 6DoF позиционирование центрального глаза HMD |
| `camera_shake` | `Game::native_camera_shake` | Снятие 10° лимита крена головы |
| `movement_input` | `Game::native_movement_input` | Ввод со стиков контроллеров и флаги прыжка/приседания |
| `hand_pose` | `Game::native_render_hand_pose` | Позиционирование 3D моделей рук и предметов в руках |
| `world_primitives`| `Game::native_world_primitives` | Рендер дуги телепорта, лазера, 3D GUI, клавиатуры и HUD |
| `client_packet` | `Game::native_client_packet` | Перехват сетевых пакетов |
| `custom_payload` | `Game::native_custom_payload_from_client` | Обработка пакетов канала `vivecraft:data` |
| `key` | `Game::native_key` | Хоткеи `F8` (Recenter) и `T` (Teleport) |
| `settings` | `Game::native_settings` | VR-вкладка в настройках Horizon |
| `click` | `Game::native_settings_click` | Клик по настройкам VR |
| `zoom` | `Game::native_zoom` | Оптический зум подзорной трубы `TelescopeTracker` |
| `arrow_nock` | `Game::native_arrow_nock` | Фиксация натяжения лука |
| `arrow_loose` | `Game::native_arrow_loose` | Расчет импульса выстрела из лука |
| `player_submit` | `Game::native_player_submit` | Рендеринг 3D поз других VR игроков |
| `client_attack` | `Game::native_client_attack` | Физический взмах оружием со скоростью $>2.5$ м/с |
| `screen_mouse` | `Game::native_screen_mouse_button_pressed_pre` | Лазерный клик по летающим GUI меню |
| `screen_char` | `Game::native_screen_char_typed_pre` | Набор текста с 3D клавиатуры |
| `menu_background`| `Game::native_draw_menu_background` | Рендер живых 3D Menu Worlds диорам |

> [!NOTE]
> При расширении количества инъекций файл `tools/test_stage.py` (строка 55) обновляется для проверки актуального числа инъекций.

---

## Этап 10. Комплексный план верификации, автотесты и приемочный чек-лист

### 10.1. Автоматизированные тесты ядра
```sh
# Запуск изолированных unit-тестов математики, матриц, протокола и трекеров
cargo test -p vivecraft-vr --manifest-path mixins/vivecraft/horizon-vr/Cargo.toml
```
- [ ] Тесты загрузчика OpenXR, структур и создания пространств.
- [ ] Тесты генерации асимметричных Reversed-Z матриц проекции для шлемов с разным FOV.
- [ ] Тесты алгоритмов DDA воксельного рейкастинга для блоков и сущностей.
- [ ] Тесты всех 10 трекеров (Climb, Swim, Row, Jump, Horse, Vehicle, Crawl, Sneak, Bow, Swing).
- [ ] Тесты формул расчета рта, прицеливания лука и натяжения поводьев лошади.
- [ ] Тесты бинарной кодировки и парсинга всех 32 пакетов протокола `vivecraft:data`.

### 10.2. Тестирование интеграции и валидация AST Horizon
```sh
python3 tools/test_stage.py
python3 tools/horizon.py stage --horizon ./horizon
python3 tools/horizon.py check --horizon ./horizon
python3 tools/package.py
```
- [ ] Валидация отсутствия синтаксических ошибок инъекций миксина (schema 6).
- [ ] Проверка компиляции результирующего бинарного мода.

### 10.3. Матрица ручного приемочного тестирования

| Подсистема | Тестовый сценарий | Ожидаемый результат | Статус |
| :--- | :--- | :--- | :---: |
| **OpenXR** | Старт без шлема | Активация `NullVR`, управление мышью, 6DoF симуляция | [ ] |
| **OpenXR** | Старт со шлемом (Meta Quest / SteamVR) | OpenXR сессия переходит в `FOCUSED`, позы HMD и контроллеров активны | [ ] |
| **OpenXR** | Тактильная отдача | Вибрация при выстреле из лука, ударе мечом, поклевке рыбы, закрытии дверей | [ ] |
| **Рендеринг** | Стереоскопия в шлеме | Независимый рендер для обоих глаз с корректной глубиной и Reversed-Z | [ ] |
| **Рендеринг** | Крен головы (Roll) | Свободный наклон головы влево/вправо без ограничения 10° | [ ] |
| **Рендеринг** | Stencil & Vignette | Маска скрытых областей отсекает края линз; виньетка снижает укачивание | [ ] |
| **Рендеринг** | Mirror View | На мониторе ПК отображается левый глаз или Side-by-Side режим | [ ] |
| **Локомоция** | Плавное движение и поворот | Левый стик — ходьба по направлению контроллера/головы; правый — snap/smooth поворот | [ ] |
| **Локомоция** | Параболический телепорт | Зажатие кнопки рисует дугу с кругом приземления, отпускание мгновенно телепортирует | [ ] |
| **Локомоция** | Физика Roomscale в комнате | Игрок физически шагает в комнате: не проходит сквозь стены, автоматически поднимается на ступени | [ ] |
| **Локомоция** | Приседания и ползание | Присед ниже 1.5м включает скрытность; наклон ниже 0.75м переводит в плавание/ползание | [ ] |
| **Локомоция** | Лестницы, брасс, лодка | Захват ступеней руками поднимает игрока; взмахи в воде плывут; взмахи в лодке гребут | [ ] |
| **Локомоция** | Верховая езда | Натяжение поводьев поворачивает лошадь; хлыст ускоряет; шаг из седла спешивает | [ ] |
| **Интеракции**| Двуручный лук | Натяжение тетивы двумя руками с вибрацией; точный выстрел при отпускании | [ ] |
| **Интеракции**| Ближний бой | Размах оружием со скоростью $>2.5$ м/с наносит урон и спавнит sweep-партиклы | [ ] |
| **Интеракции**| Физический щит | Подставление щита под стрелы физически отбивает их без нажатия кнопок | [ ] |
| **Интеракции**| Еда и зелья | Поднесение еды ко рту автоматически активирует поедание с хрустом | [ ] |
| **Интеракции**| Полет на элитрах | Планирование с расправленными руками; наклон рук управляет траекторией полета | [ ] |
| **Интеракции**| Удочка | Леска выходит из кончика удочки; поклевка передает виброотклик в руку | [ ] |
| **3D Аудио**  | HRTF слушатель | Источники звука позиционируются относительно положения и наклона HMD | [ ] |
| **3D GUI**    | Меню и сундуки | Интерфейс висит в 3D пространстве; лазерный луч управляет курсором | [ ] |
| **3D GUI**    | Радиальное меню | Быстрый выбор 8 действий наклоном стика с виброщелчками | [ ] |
| **3D GUI**    | 3D Клавиатура | Набор текста в чат лазером или барабанными палочками | [ ] |
| **3D GUI**    | Наручный HUD | Поворот запястья отображает сердечки, голод и броню на руке | [ ] |
| **3D GUI**    | 3D Menu Worlds | В главном меню игрок находится внутри живой трехмерной сцены | [ ] |
| **Сеть**      | Серверный AimFix | Стрелы, снежки и блоки спавнятся точно из контроллера на сервере | [ ] |
| **Сеть**      | Серверные хэдшоты | Стрела в голову моба дает 3.0x урон, крит-партиклы и характерный «динь» | [ ] |
| **Сеть**      | Мультиплеер | Другие VR-игроки видны в полном 6DoF (движения рук, головы, элитр) | [ ] |
| **Предметы**  | Когти и Ботинки | Крафт Climb Claws и Jump Boots работает; когти позволяют лазать по скалам | [ ] |
