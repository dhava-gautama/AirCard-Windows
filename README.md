# AirCard (Windows)

**Apple Wallet card skinner & lockscreen passcode themer for iOS 18+ (no jailbreak).** Native Rust client. This public fork keeps Lumid-Off’s Windows port and adds a working AirTraffic handshake on Windows (including **iOS 26.x**).

**Tutorial Bahasa Indonesia:** [TUTORIAL.id.md](TUTORIAL.id.md)

> Unofficial. Back up the iPhone first. Apple can patch this path; skins can vanish after an iOS update.

## What this fork fixes

Upstream `aircard.exe` on Windows often dies with **AirTraffic sync timed out** or **`SyncFailed` ErrorCode 4**. The host sent a broken Grappa blob. This tree:

1. Uses `ATHostConnectionCreateWithLibrary` + `RequestingSync` with a replayable Grappa handshake.
2. Batches Wallet (and passcode) file writes into **one** Books sync instead of one handshake per file.

Verified on **iPhone 18,2 / iOS 26.6** over USB.

## Features

- Custom Apple Pay / Apple Cash card artwork (PNG / JPG / WebP / **PDF** for Suica-style art)
- Pan / zoom crop before flash
- Named saved cards, last-image reapply, revert previous AirCard skin
- Cowabunga / Nugget `.passthm` lockscreen keypad themes, including **Bold Text** (`--white-bold`) caches
- Mac-style keypad creator: poster-slice a wallpaper, or load `0.png`…`9.png`
- UI language: EN / ID
- iTunes / Apple Devices warning before AirTraffic
- Single native `aircard.exe` (egui), no Python runtime
- Syslog card-hash scan while you tap a card in Wallet
- Books state snapshot + restore after the write

## Requirements

- Windows 10 / 11 (64-bit)
- [Apple Mobile Device Support](https://support.apple.com/en-us/HT210384) or the **Apple Devices** / 64-bit iTunes package
- Lightning or USB-C **data** cable

Quit **iTunes** and **Apple Devices** while flashing. Keep the iPhone unlocked; open **Books** once.

## Install / build

```powershell
git clone https://github.com/dhava-gautama/AirCard-Windows.git
cd AirCard-Windows
cargo test
cargo build --release
```

Binary: `target\release\aircard.exe`.

CLI:

```powershell
.\target\release\aircard.exe --probe
.\target\release\aircard.exe --flash "CARD_HASH" "D:\art.png"
.\target\release\aircard.exe --flash "CARD_HASH" "D:\suica.pdf"
.\target\release\aircard.exe --passcode "D:\theme.passthm"
.\target\release\aircard.exe --passcode "D:\theme.passthm" TelephonyUI-10 English
```

### AirTraffic handshake probe

Run `aircard.exe --probe` from PowerShell or cmd with one USB-connected, unlocked iPhone; open Books once beforehand. The probe tests `SyncAllowed` → `RequestingSync` → `ReadyForSync` and then releases the connection. It does not run AFC cleanup, stage files, or write Wallet assets. It still sends the handshake messages to the device. The command attaches to the invoking Windows console to print progress and, on `SyncFailed`, only a numeric `ErrorCode` when available. `AIRCARD_PROBE_ONLY=1` is kept as an alias (including with leftover `--flash` args). In the GUI, **Probe** runs the same handshake on the selected phone. Normal GUI launch remains windowed.

**Additional single-device validation:** Manually verified on Windows with iPhone15,4 / iOS 18.7.1. Lumid-Off v1.2.2 reproduced `SyncFailed` ErrorCode 4; this fork's handshake probe reached `ReadyForSync`, and a subsequent full Wallet artwork application succeeded. This is one real-device observation, not a claim of support for every iOS 18.x device or version. The Grappa handshake fix was already present in this fork; the probe and narrower failure logging do not introduce that fix.

## Wallet skins

1. USB, unlocked, **Trust this Computer**.
2. Wallet tab → **Scan**. Open Wallet (or double-click Side button), tap the card, **Stop**.
3. **Choose Image...** (PNG / JPG / WebP / PDF). Use the frame sliders to pan/zoom. Saved cards can be renamed; **Apply last image** reloads the path you used last.
4. **Apply Card Skin**. **Revert last AirCard skin** restores the previous PNG this app stored for that hash (apply twice to have a previous copy).
5. Force-close Wallet on the iPhone and reopen it.

## Passcode themes

1. Passcode tab → choose a `.passthm`, **or** slice a wallpaper / load a folder of `0.png`…`9.png`.
2. Cache: **TelephonyUI-10** (iOS 18+), **9** (16–17), **8** (legacy).
3. **Apply Passcode Theme**. Regular and **Bold Text** caches (`--white.png` and `--white-bold.png`) are both written.
4. Lock the phone. Bold Text can stay ON.

## Troubleshooting

| Symptom | Try |
|--------|-----|
| No device | Cable, Trust, Apple Mobile Device Service running |
| Stuck on SyncAllowed | Unlocked screen, open Books once |
| SyncFailed / 35s timeout | This fork, not the old Lumid-Off release; quit iTunes; run `--probe` first |
| Skin not visible | Force-close Wallet or reboot |

If the iPhone is still missing after that, Apple USB drivers on Windows are often the cause. Optional last resort:

1. Disconnect the iPhone.
2. Install **[3uTools](https://www.3u.com/)** → **Toolbox → Repair Driver → Repair Now**.
3. Reconnect, tap **Trust**, launch AirCard.

3uTools is third-party (not Apple). Prefer repairing with Apple Mobile Device Support first.

## Credits

- Windows port: [@Lumid-Off](https://github.com/Lumid-Off) — [Lumid-Off/AirCard-Windows](https://github.com/Lumid-Off/AirCard-Windows)
- macOS app: [@mak5er](https://github.com/mak5er)
- Windows Grappa handshake + batch flash: [@dhava-gautama](https://github.com/dhava-gautama)
- Core write path: `airlift` (AirTraffic Books sync)
- Theme format: [Cowabunga](https://github.com/leminlimez/Cowabunga) / [Nugget](https://github.com/leminlimez/Nugget)

MIT — see [LICENSE](LICENSE).
