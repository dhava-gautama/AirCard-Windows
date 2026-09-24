# Tutorial AirCard (Windows) — Bahasa Indonesia

Ganti gambar kartu Apple Pay / Apple Cash dan tema tombol kode sandi iPhone **tanpa jailbreak**. Tutorial ini untuk fork Windows yang sudah diperbaiki agar sinkronisasi AirTraffic berfungsi di **iOS 18+ termasuk iOS 26.x**.

> Bukan aplikasi resmi Apple. Cadangkan iPhone dulu. Hasil bisa hilang setelah update iOS.

## Apa yang bisa dan tidak bisa

**Bisa**
- Ganti artwork kartu di Apple Wallet
- **Simpan asli** / **Pulihkan asli** (ekspor Airlift; cadangan tidak pernah ditimpa)
- Ukuran Wallet benar: @3x 1536×969 dan @2x **1024×646**
- Pasang tema keypad `.passthm` (Cowabunga / Nugget)
- iPhone tersambung USB ke PC Windows 10/11 64-bit

**Tidak**
- Bukan jailbreak, bukan bypass iCloud, bukan ubah sistem partisi
- Tidak mengubah nomor kartu, saldo, atau data bank
- Tidak dijamin di semua versi iOS; Apple bisa menambal jalur ini kapan saja

## Persiapan

1. **PC:** Windows 10 atau 11 (64-bit).
2. **Driver Apple:** pasang [Apple Mobile Device Support](https://support.apple.com/en-us/HT210384) atau aplikasi **Apple Devices** / iTunes 64-bit.
3. **Kabel:** Lightning atau USB-C yang benar-benar data (bukan charge-only).
4. **iPhone:** buka kunci, layar tetap nyala, ketuk **Percayai Komputer Ini**.
5. Tutup **iTunes** dan **Apple Devices** di PC saat AirCard berjalan (mereka bisa merebut layanan AirTraffic).
6. Buka aplikasi **Buku** (Apple Books) di iPhone sekali, lalu biarkan.

Build rilis:

```powershell
git clone https://github.com/dhava-gautama/AirCard-Windows.git
cd AirCard-Windows
cargo test
cargo build --release
```

File hasil: `target\release\aircard.exe`.

## Ganti kulit kartu Wallet

1. Sambungkan iPhone, pastikan terbuka dan dipercaya.
2. Jalankan `aircard.exe`, tetap di tab **Wallet**, klik **Scan**.
3. Di iPhone:
   - Buka **Wallet**, atau klik dua kali tombol samping untuk Apple Pay.
   - Ketuk kartu yang ingin diubah.
   - Kartu transit di iOS 27: buka kartu → **…** → **Detail Kartu** → **Nyalakan Mode Layanan**, lalu Scan.
   - AirCard menyimpan hash kartu. Klik **Stop**.
4. Klik **Simpan asli** **sebelum** kulit pertama. Cadangan per perangkat ada di `%LOCALAPPDATA%\AirCard\wallet-originals` dan tidak pernah ditimpa. **Pulihkan asli** mengembalikan artwork Apple itu. **Kembalikan kulit AirCard terakhir** hanya membatalkan PNG AirCard sebelumnya.
5. **Choose Image...** — PNG, JPG, WebP, atau PDF. Geser zoom/pan jika perlu. Kartu tersimpan bisa diganti nama; **Pasang gambar terakhir** membuka file yang tadi dipakai.
6. **Pasang kulit kartu**. Tulisan memakai @3x 1536×969 dan @2x 1024×646, lalu memindahkan cache `FrontFace` / `PlaceHolder` / `Preview` agar iOS 27 tidak menampilkan pratinjau lama.
7. Di iPhone, **paksa tutup Wallet** (App Switcher: geser Wallet ke atas) lalu buka lagi.

### Lewat command line

```powershell
.\aircard.exe --probe
.\aircard.exe --flash "HASH_KARTU_DI_SINI" "D:\gambar.png"
.\aircard.exe --flash "HASH_KARTU_DI_SINI" "D:\suica.pdf"
.\aircard.exe --save-original "HASH_KARTU_DI_SINI"
.\aircard.exe --restore-original "HASH_KARTU_DI_SINI"
.\aircard.exe --passcode "D:\tema.passthm"
```

`--probe` hanya menguji handshake AirTraffic (`ReadyForSync`). Tidak menulis kulit Wallet. Di jendela aplikasi, tombol **Probe** melakukan hal yang sama.

Hash contoh terlihat seperti `k6pyiSrrP1J2v3t51G1sEDDnOZo=` (hasil Scan, bukan nomor kartu bank).

## Tema keypad kode sandi

1. Tab **Passcode**.
2. Pilih file `.passthm`, **atau** potong wallpaper menjadi 10 tombol bundar, **atau** folder berisi `0.png`…`9.png`.
3. Cache iOS:
   - **Auto (TelephonyUI-10)** — iOS 18+
   - **TelephonyUI-9** — iOS 16–17
   - **TelephonyUI-8** — iOS lama
4. **Apply Passcode Theme**. Cache **Teks Tebal** (`--white-bold`) ikut ditulis.
5. Kunci layar iPhone untuk melihat tombol baru. Teks Tebal boleh tetap nyala.

## Kalau gagal

| Gejala | Yang dicoba |
|--------|-------------|
| iPhone tidak terdeteksi | Kabel lain, Trust ulang, pastikan layanan Apple Mobile Device berjalan |
| `SyncAllowed` tidak muncul | Buka kunci, layar nyala, buka **Buku** sekali |
| SyncFailed / timeout 35 detik | Pakai build fork ini (bukan `aircard.exe` rilis Lumid-Off lama). Tutup iTunes / Apple Devices. Jalankan `--probe` dulu |
| Kulit tidak kelihatan | Paksa tutup Wallet, atau reboot iPhone. Di iOS 27 cache wajah dipindah, bukan ditimpa |
| Hash kartu transit tidak ketemu | Wallet → kartu → … → Detail Kartu → Nyalakan Mode Layanan, lalu Pindai |
| Pulihkan asli tidak aktif | Klik **Simpan asli** sebelum kulit kustom pertama |
| Tema passcode tidak kelihatan | Kunci layar ulang; pastikan cache `--white-bold` ikut (build 1.3+) |

Kalau iPhone masih tidak muncul, driver USB Apple di Windows sering rusak. Langkah opsional:

1. Cabut iPhone.
2. Pasang **[3uTools](https://www.3u.com/)** → **Toolbox → Repair Driver → Repair Now**.
3. Colok lagi, ketuk **Percayai**, buka AirCard.

3uTools bukan software Apple. Utamakan pasang ulang **Apple Mobile Device Support** dulu.

## Kredit

- Port Windows asli: [Lumid-Off/AirCard-Windows](https://github.com/Lumid-Off/AirCard-Windows)
- Aplikasi macOS: [Mak5er/AirCard](https://github.com/Mak5er/AirCard)
- Jalur tulis: `airlift` (AirTraffic / Books)
- Perbaikan handshake Windows (Grappa) + batch flash: fork ini ([dhava-gautama/AirCard-Windows](https://github.com/dhava-gautama/AirCard-Windows))
