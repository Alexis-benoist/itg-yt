# itg-yt

URL YouTube → dossier de chanson ITGmania complet (charts, OGG, visuels, vidéo de fond).
Projet séparé d'itg-charter (`~/itg-charter`, https://github.com/Alexis-benoist/itg-charter),
utilisé **uniquement comme programme** (`itg-charter gen`) : ne pas ajouter de dépendance Rust vers
lui (il compile aubio et embarque un modèle ; ici on ne lit que quelques tags du `.sm`).

## Commandes

- `cargo build --release` → `target/release/itg-yt`.
- `cargo test --release` : tests unitaires + `tests/pipeline.rs`, hors-ligne. Un faux yt-dlp et un
  faux itg-charter (scripts shell générés par le test) remplacent le réseau et les charts ;
  ffmpeg/ffprobe tournent pour de vrai (le test est sauté s'ils sont absents). Les tests utilisent
  une petite vidéo et `--video-height 144 --video-preset ultrafast` pour rester rapides (~7 s).
- `~/.cargo/bin/cargo fmt --all` et `~/.cargo/bin/cargo clippy --all-targets -- -D warnings`
  (la toolchain Ubuntu n'a ni rustfmt ni clippy ; la CI les exige).

## Règles

- **Valeurs par défaut complètes** : 5 difficultés et vidéo de fond activées ; les options servent à
  retirer (`--no-video`, `-d`), pas à ajouter.
- **Parallélisme** : téléchargements simultanés ; encodages simultanés ; les charts démarrent dès
  que l'OGG existe ; la vidéo (x264, long) tourne en `nice` pour ne pas retarder le reste.
- Les charts sont générés sur l'**OGG final** (les octets que le jeu lit) : c'est ce qui garantit
  la synchro et, avec le cache de téléchargement, la reproductibilité (même vidéo + même seed ⇒
  même `.sm`). Les encodages utilisent `-bitexact`.
- Le nom du dossier doit rester calculé comme dans itg-charter (`sanitize` + remplacement de
  `/?*"<>|`) : `itg-yt` vérifie que le `.sm` est bien écrit dans son dossier.
- `#BGCHANGES` : la vidéo démarre au beat `OFFSET × BPM / 60`, c'est-à-dire au temps 0 de l'audio
  (itg-charter écrit un BPM constant). Format du champ relevé dans des packs réels.
- yt-dlp a besoin des scripts EJS et d'un runtime JS (`uv tool install "yt-dlp[default]"`). Si
  deno est absent et node présent, on passe `--js-runtimes node`.
