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

- Custom Apple Pay / Apple Cash card artwork
- Cowabunga / Nugget `.passthm` lockscreen keypad themes
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
.\target\release\aircard.exe --flash "CARD_HASH" "D:\art.png"
```

## Wallet skins

1. USB, unlocked, **Trust this Computer**.
2. Wallet tab → **Scan**. Open Wallet (or double-click Side button), tap the card, **Stop**.
3. **Choose Image...** (PNG / JPG / WebP → `1536 × 969`).
4. **Apply Card Skin**.
5. Force-close Wallet on the iPhone and reopen it.

## Passcode themes

1. Passcode tab → choose a `.passthm`.
2. Cache: **TelephonyUI-10** (iOS 18+), **9** (16–17), **8** (legacy).
3. **Apply Passcode Theme**. Turn **Bold Text OFF** (Settings → Display & Brightness) or iOS ignores keypad bitmaps.

## Troubleshooting

| Symptom | Try |
|--------|-----|
| No device | Cable, Trust, Apple Mobile Device Service running |
| Stuck on SyncAllowed | Unlocked screen, open Books once |
| SyncFailed / 35s timeout | This fork, not the old Lumid-Off release; quit iTunes |
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
