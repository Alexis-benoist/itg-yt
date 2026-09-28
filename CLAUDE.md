# itg-yt

URL YouTube → dossier de chanson ITGmania complet (charts, OGG, visuels, vidéo de fond).
Projet séparé d'itg-charter (https://github.com/Alexis-benoist/itg-charter), utilisé
**comme bibliothèque** (dépendance git dans `Cargo.toml`, version figée par `Cargo.lock` ;
`cargo update -p itg-charter` pour suivre `main`). Tout ce qui concerne le dossier de chanson et le
`.sm` appartient à itg-charter (`song::create_song`, `song::decorate`, `song::Charts` /
`Profile` / `parse_meters`, `chart::assign_slots`) ; itg-yt ne lit ni n'écrit le `.sm`.
`.cargo/config.toml` (CFLAGS pour le C d'aubio, compilé via itg-charter) est indispensable.

## Commandes

- `cargo build --release` → `target/release/itg-yt`.
- `cargo test --release` (~5 s) : tests unitaires + `tests/pipeline.rs`, hors-ligne. Un faux yt-dlp
  sert des médias générés (échecs simulés par des fichiers jetons `crash-*`, consommés par `rm`
  atomique), ffmpeg tourne pour de vrai via un wrapper qui compte les encodages, et les charts sont
  générés par la vraie bibliothèque itg-charter en `--no-stems`. Vidéo en
  `--video-height 144 --video-preset ultrafast` pour rester rapide.
- `~/.cargo/bin/cargo fmt --all` et `~/.cargo/bin/cargo clippy --all-targets -- -D warnings`
  (la toolchain Ubuntu n'a ni rustfmt ni clippy ; la CI les exige).
- Releases : automatiques à chaque push sur `main` (`vMAJOR.MINOR.<run>`), ou sur un tag `v*` ;
  jobs `version` → `build` (matrice, fail-fast off) → `release` (un seul, publie ce qui a compilé).

## Règles

- **Valeurs par défaut complètes** : profil par défaut d'itg-charter, Demucs et vidéo de fond activés ; les options
  servent à retirer (`--no-video`, `--no-stems`, `-d`), pas à ajouter.
- **Niveaux** : `-p/--profile`, `-m/--meters`, `-d/--difficulties` comme `itg-charter gen` ;
  validés (plage 1–10, 5 au plus) avant tout téléchargement.
- **Plusieurs URL** : métadonnées en un seul `yt-dlp -j --ignore-errors` (playlists développées,
  `--no-playlist` garde une vidéo pour `watch?v=…&list=…`), doublons retirés par id. Pool de
  `--jobs` workers ; `song::create_song` est protégé par un mutex (un seul Demucs sur le GPU). Une
  chanson en échec n'arrête pas les autres ; code de retour 1 s'il y a eu un échec.
- **Relances** : options de relance internes de yt-dlp + `run_with_retries` (processus relancé
  jusqu'à `--yt-retries`, pauses 2/4/8 s ; `ITG_YT_RETRY_PAUSE_MS` pour les tests) ; l'appel de
  métadonnées est refait tant qu'il reste des erreurs.
- **Parallélisme** : téléchargements simultanés ; encodages simultanés ; `create_song` démarre dès que
  l'OGG et les images existent ; la vidéo (x264, long) tourne en `nice` et n'est attendue que pour
  `song::decorate`.
- **Cache** (`~/.cache/itg-charter/youtube/<id>/`) : téléchargements, puis `encoded/` (OGG, images,
  vidéo par `video-<h>p-<preset>-crf<n>/`). Les encodages sont écrits sous un nom `.partial` puis
  renommés : jamais de fichier tronqué réutilisé.
- **Marquage** : les `.sm` produits portent « itg-charter » dans `#CREDIT` et la description des
  charts (écrits par itg-charter). Ne pas le retirer : `~/ITG-YouTube` est lié dans le dossier Songs
  du jeu, et itg-charter exclut ces fichiers de ses données d'entraînement / d'évaluation par ce
  marquage (`Simfile::is_generated`). Vérifié par `builds_a_complete_song_folder`.
- Les charts sont générés sur l'**OGG final** (les octets que le jeu lit) : synchro juste et
  reproductibilité (même vidéo + même seed ⇒ même `.sm`). Encodages en `-bitexact`.
- yt-dlp a besoin des scripts EJS et d'un runtime JS (`uv tool install "yt-dlp[default]"`). Si
  deno est absent et node présent, on passe `--js-runtimes node`.
