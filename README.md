<p align="center">
  <img src="assets/icon/gyozarec-256.png" width="256" height="256" alt="GyozaRec">
</p>

<h1 align="center">GyozaRec</h1>

<p align="center">Cattura schermo veloce e carina: PNG, GIF e MP4, tutto in locale.</p>

## Funzionalità

Una piccola barra con quattro pulsanti:

- **Cattura** — avvia la cattura con la modalità e il ritardo scelti.
- **Modalità** — Rettangolo, Finestra, Schermo intero, Video MP4, GIF animata.
- **Ritarda** — da nessun ritardo fino a 5 secondi.
- **Opzioni** — cartella di salvataggio, copia negli appunti, suono, cursore nei video, fotogrammi al secondo e durata massima di GIF e video.

Le immagini vengono salvate in PNG e copiate negli appunti. Video e GIF registrano un'area scelta sullo schermo; durante la registrazione un pannello mostra il tempo trascorso con i pulsanti **Stop** e **Annulla**. Nessun file lascia il computer: niente upload, niente account.

Avviando `gyozarec --capture` la selezione parte subito; se l'app è già aperta, la cattura parte nella finestra esistente.

## Download

Dalla pagina Releases del repository:

| Pacchetto | Cosa fa |
|---|---|
| `GyozaRec_<versione>_x64-setup.exe` | Installazione per l'utente corrente, senza permessi di amministratore. Impostazioni in `%APPDATA%\GyozaRec`, catture in `Immagini\GyozaRec`. |
| `GyozaRec-<versione>-windows-x64-portable.zip` | Versione portable: estrai e avvia `gyozarec.exe`. Impostazioni e catture restano nella cartella (`data\` e `Catture\`). |

Richiede Windows 10 (versione 2004 o successiva) o Windows 11, a 64 bit. macOS e Linux sono previsti nelle prossime versioni.

## Compilare

Serve Rust stabile (1.93 o successivo) con il toolchain MSVC.

```
cargo build --release
cargo test
```

Per creare installer e zip portable (richiede `cargo install cargo-packager --locked`):

```
cargo xtask dist
```

## Licenza

MIT, vedi [`LICENSE`](LICENSE). Il font Nunito è distribuito sotto SIL Open Font License, vedi [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt).
