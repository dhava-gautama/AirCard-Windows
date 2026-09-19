# Tutorial AirCard (Windows) — Bahasa Indonesia

Ganti gambar kartu Apple Pay / Apple Cash dan tema tombol kode sandi iPhone **tanpa jailbreak**. Tutorial ini untuk fork Windows yang sudah diperbaiki agar sinkronisasi AirTraffic berfungsi di **iOS 18+ termasuk iOS 26.x**.

> Bukan aplikasi resmi Apple. Cadangkan iPhone dulu. Hasil bisa hilang setelah update iOS.

## Apa yang bisa dan tidak bisa

**Bisa**
- Ganti artwork kartu di Apple Wallet
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
   - AirCard menyimpan hash kartu. Klik **Stop**.
4. **Choose Image...** — PNG, JPG, atau WebP. Gambar dipotong tengah dan diubah ke `1536 × 969`.
5. **Apply Card Skin**. Tunggu sampai selesai (satu sesi sinkron, bukan delapan kali).
6. Di iPhone, **paksa tutup Wallet** (App Switcher: geser Wallet ke atas) lalu buka lagi.

### Lewat command line

```powershell
.\aircard.exe --flash "HASH_KARTU_DI_SINI" "D:\gambar.png"
```

Hash contoh terlihat seperti `k6pyiSrrP1J2v3t51G1sEDDnOZo=` (hasil Scan, bukan nomor kartu bank).

## Tema keypad kode sandi

1. Tab **Passcode**.
2. Pilih file `.passthm`.
3. Cache iOS:
   - **Auto (TelephonyUI-10)** — iOS 18+
   - **TelephonyUI-9** — iOS 16–17
   - **TelephonyUI-8** — iOS lama
4. **Apply Passcode Theme**.
5. Kunci layar iPhone untuk melihat tombol baru.

**Teks Tebal harus MATI:** Pengaturan → Tampilan & Kecerahan → **Teks Tebal** off. Kalau nyala, iOS menggambar font sistem dan mengabaikan gambar tombol.

## Kalau gagal

| Gejala | Yang dicoba |
|--------|-------------|
| iPhone tidak terdeteksi | Kabel lain, Trust ulang, pastikan layanan Apple Mobile Device berjalan |
| `SyncAllowed` tidak muncul | Buka kunci, layar nyala, buka **Buku** sekali |
| `SyncFailed` / timeout 35 detik | Pakai build fork ini (bukan `aircard.exe` rilis Lumid-Off lama). Tutup iTunes / Apple Devices |
| Kulit tidak kelihatan | Paksa tutup Wallet, atau reboot iPhone |
| Tema passcode tidak kelihatan | Matikan Teks Tebal, kunci layar ulang |

Jangan unduh “perbaikan driver” dari situs random. Kalau driver Apple rusak, pasang ulang **Apple Mobile Device Support** dari Apple.

## Kredit

- Port Windows asli: [Lumid-Off/AirCard-Windows](https://github.com/Lumid-Off/AirCard-Windows)
- Aplikasi macOS: [Mak5er/AirCard](https://github.com/Mak5er/AirCard)
- Jalur tulis: `airlift` (AirTraffic / Books)
- Perbaikan handshake Windows (Grappa) + batch flash: fork ini ([dhava-gautama/AirCard-Windows](https://github.com/dhava-gautama/AirCard-Windows))
