OpenXR loader libraries shipped with the vivecraft bundle.

The game's loader search order (xr.rs `loader_candidates`):
1. SteamVR installs: Steam's folder from the registry
   (`HKCU\Software\Valve\Steam\SteamPath`, `HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath`,
   then a hardcoded `C:\Program Files (x86)\Steam`), each as
   `<steam>\steamapps\common\SteamVR\bin\win64\`;
2. this bundle's staged `native-data/vivecraft/` beside the game executable
   (what the post-build step below fills);
3. the system PATH.

The post-build step (src/bin/copy_openxr_loader.rs of the vr_engine crate) copies
whatever is here into the build's native-data/vivecraft/ beside the game
executable.

openxr_loader.dll is NOT committed to this repository. To ship it in a built .hnmod,
place it here, e.g. copied from a SteamVR install:

  C:\Program Files (x86)\Steam\steamapps\common\SteamVR\bin\win64\openxr_loader.dll

(Linux: libopenxr_loader.so.1, macOS: libopenxr_loader.dylib, from the runtime
vendor.)
