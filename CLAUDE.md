# itg-yt

URL YouTube → dossier de chanson ITGmania complet (charts, OGG, visuels, vidéo de fond).
Projet séparé d'itg-charter (`~/itg-charter`, https://github.com/Alexis-benoist/itg-charter),
utilisé **uniquement comme programme** : ne pas ajouter de dépendance Rust vers lui (il compile
aubio et embarque un modèle). Tout ce qui concerne le dossier de chanson et le `.sm` (nom du dossier,
tags, `#BGCHANGES`) appartient à itg-charter (`gen --banner/--background/--jacket`,
`decorate --bg-video`) ; itg-yt ne lit ni n'écrit le `.sm`.

## Commandes

- `cargo build --release` → `target/release/itg-yt`.
- `cargo test --release` (~2 s) : tests unitaires + `tests/pipeline.rs`, hors-ligne. Un faux yt-dlp
  sert des médias générés, ffmpeg tourne pour de vrai via un wrapper qui compte les encodages, un faux
  itg-charter journalise ses appels. `with_the_real_itg_charter` utilise le vrai binaire
  (`~/itg-charter/target/release/itg-charter` ou `ITG_CHARTER_REAL`) quand il existe, sinon il est
  sauté (cas de la CI). Les tests utilisent `--video-height 144 --video-preset ultrafast`.
- `~/.cargo/bin/cargo fmt --all` et `~/.cargo/bin/cargo clippy --all-targets -- -D warnings`
  (la toolchain Ubuntu n'a ni rustfmt ni clippy ; la CI les exige).
- Release : pousser un tag `vX.Y.Z` (la CI publie le binaire).

## Règles

- **Valeurs par défaut complètes** : 5 difficultés, Demucs et vidéo de fond activés ; les options
  servent à retirer (`--no-video`, `--no-stems`, `-d`), pas à ajouter.
- **Plusieurs URL** : métadonnées en un seul `yt-dlp -j --ignore-errors` (playlists développées,
  `--no-playlist` garde une vidéo pour `watch?v=…&list=…`), doublons retirés par id. Pool de
  `--jobs` workers ; `itg-charter gen` est protégé par un mutex (un seul Demucs sur le GPU). Une
  chanson en échec n'arrête pas les autres ; code de retour 1 s'il y a eu un échec.
- **Relances** : options de relance internes de yt-dlp + `run_with_retries` (processus relancé
  jusqu'à `--yt-retries`, pauses 2/4/8 s ; `ITG_YT_RETRY_PAUSE_MS` pour les tests) ; l'appel de
  métadonnées est refait tant qu'il reste des erreurs.
- **Parallélisme** : téléchargements simultanés ; encodages simultanés ; `gen` démarre dès que
  l'OGG et les images existent ; la vidéo (x264, long) tourne en `nice` et n'est attendue que pour
  `decorate`.
- **Cache** (`~/.cache/itg-charter/youtube/<id>/`) : téléchargements, puis `encoded/` (OGG, images,
  vidéo par `video-<h>p-<preset>-crf<n>/`). Les encodages sont écrits sous un nom `.partial` puis
  renommés : jamais de fichier tronqué réutilisé.
- Les charts sont générés sur l'**OGG final** (les octets que le jeu lit) : synchro juste et
  reproductibilité (même vidéo + même seed ⇒ même `.sm`). Encodages en `-bitexact`.
- yt-dlp a besoin des scripts EJS et d'un runtime JS (`uv tool install "yt-dlp[default]"`). Si
  deno est absent et node présent, on passe `--js-runtimes node`.
