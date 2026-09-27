# itg-yt

Crée un dossier de chanson **ITGmania / In The Groove** complet à partir d'une vidéo YouTube :
charts (via [itg-charter](https://github.com/Alexis-benoist/itg-charter)), audio OGG, bannière,
fond, jaquette et clip en fond animé.

```sh
itg-yt "https://www.youtube.com/watch?v=..."
itg-yt URL1 URL2 URL3                 # plusieurs chansons
itg-yt "https://www.youtube.com/playlist?list=..."   # toute une playlist
itg-yt -a mes-chansons.txt            # une URL par ligne (# = commentaire, - = stdin)
```

- [Installation](#installation) : itg-yt, ffmpeg, yt-dlp, Demucs, branchement sur le jeu
- [Utilisation](#utilisation) · [Options](#options) · [Fonctionnement](#fonctionnement)
- [Dépannage](#dépannage)

## Installation

### Ce qu'il faut

| outil | rôle | obligatoire ? |
|---|---|---|
| **itg-yt** | ce programme (itg-charter est compilé dedans : rien d'autre à installer pour les charts) | oui |
| **ffmpeg** avec libvorbis et libx264 | encodage de l'audio (OGG), des images et de la vidéo | oui |
| **yt-dlp** avec ses scripts EJS | téléchargement depuis YouTube | oui |
| **deno** ou **node** | runtime JavaScript demandé par yt-dlp pour résoudre les challenges YouTube | oui |
| **Python 3.11 + Demucs** | sépare batterie / basse / voix pour mieux placer les notes | non (sans lui : avertissement, charts sur le mix seul) |
| **GPU NVIDIA (CUDA)** | Demucs en ~20 s par morceau au lieu de quelques minutes | non (`--device cpu`) |

Le plus simple pour yt-dlp et Demucs est [uv](https://docs.astral.sh/uv/) :

```sh
# Linux / macOS
curl -LsSf https://astral.sh/uv/install.sh | sh
```

```powershell
# Windows (PowerShell)
winget install astral-sh.uv
```

### 1. itg-yt

**Binaire tout prêt** : sur la page
[Releases](https://github.com/Alexis-benoist/itg-yt/releases) (dernière release = dernier
commit de `main`), prendre l'archive de son système :

| système | archive |
|---|---|
| Linux x86_64 | `itg-yt-vX.Y.Z-linux-x86_64.tar.gz` |
| macOS Apple Silicon (M1…) | `itg-yt-vX.Y.Z-macos-arm64.tar.gz` |
| macOS Intel | `itg-yt-vX.Y.Z-macos-x86_64.tar.gz` |
| Windows | `itg-yt-vX.Y.Z-windows-x86_64.zip` |

```sh
# Linux / macOS : vérifier, extraire, mettre dans le PATH
sha256sum -c itg-yt-vX.Y.Z-linux-x86_64.tar.gz.sha256   # macOS : shasum -a 256 -c …
tar xzf itg-yt-vX.Y.Z-linux-x86_64.tar.gz
install -m 755 itg-yt-vX.Y.Z-linux-x86_64/itg-yt ~/.local/bin/
```

Sur macOS, le binaire n'est pas signé : au premier lancement,
`xattr -d com.apple.quarantine ~/.local/bin/itg-yt`. Sous Windows, extraire le `.zip` et mettre
`itg-yt.exe` dans un dossier du `PATH` (ou le lancer depuis son dossier).

**Ou depuis les sources** : il faut Rust stable ([rustup](https://rustup.rs)) et un compilateur C,
car itg-charter compile aubio :

| système | compilateur C |
|---|---|
| Debian / Ubuntu | `sudo apt install build-essential` |
| Fedora | `sudo dnf install gcc` |
| macOS | `xcode-select --install` |
| Windows | [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/), charge de travail « Développement Desktop en C++ » |

```sh
git clone https://github.com/Alexis-benoist/itg-yt
cd itg-yt
cargo install --path .        # installe itg-yt dans ~/.cargo/bin
```

Lancer la compilation **depuis le clone** : `.cargo/config.toml` y ajoute
`CFLAGS=-D_DEFAULT_SOURCE`, sans quoi le C d'aubio ne compile pas avec GCC ≥ 14. (Un
`cargo install --git …` lancé ailleurs ne lit pas ce fichier : il faut alors exporter `CFLAGS`
soi-même.)

### 2. ffmpeg

| système | commande |
|---|---|
| Debian / Ubuntu | `sudo apt install ffmpeg` |
| Fedora | `sudo dnf install ffmpeg` (dépôt RPM Fusion ; le `ffmpeg-free` de base n'a pas libx264) |
| Arch | `sudo pacman -S ffmpeg` |
| macOS | `brew install ffmpeg` |
| Windows | `winget install Gyan.FFmpeg` (build complet) |

Vérifier que les deux encodeurs sont là :

```sh
ffmpeg -hide_banner -encoders | grep -E "libvorbis|libx264"      # Windows : findstr au lieu de grep
```

### 3. yt-dlp et un runtime JavaScript

```sh
uv tool install "yt-dlp[default]"     # [default] apporte les scripts EJS
```

Puis un runtime JS, dont yt-dlp a besoin pour YouTube :

| système | commande |
|---|---|
| Linux | deno : `curl -fsSL https://deno.land/install.sh \| sh` ; ou node : `sudo apt install nodejs` |
| macOS | `brew install deno` (ou `brew install node`) |
| Windows | `winget install DenoLand.Deno` (**deno** : la bascule automatique vers node ne marche pas sous Windows) |

yt-dlp utilise deno par défaut ; si seul node est installé, itg-yt ajoute `--js-runtimes node`.
itg-yt prend `~/.local/bin/yt-dlp` en priorité (là où uv l'installe), sinon celui du `PATH`.

YouTube change souvent de protection : **mettre yt-dlp à jour** au moindre échec de
téléchargement :

```sh
uv tool upgrade yt-dlp
```

### 4. Demucs (facultatif, recommandé)

Les charts sont meilleurs quand la batterie et la basse sont séparées du reste. Sans Demucs, itg-yt
fonctionne quand même : il affiche `warning: no stems (…); analysing the full mix only` et charte
sur le mix complet (`--no-stems` fait de même, sans l'avertissement).

**Linux** (GPU NVIDIA : torch CUDA est installé par défaut) et **macOS** :

```sh
uv venv --python 3.11 ~/.local/share/itg-charter/demucs-venv
VIRTUAL_ENV=~/.local/share/itg-charter/demucs-venv uv pip install demucs torch torchaudio soundfile
```

C'est l'emplacement où itg-yt cherche Demucs par défaut. Sans GPU NVIDIA (ce qui inclut tous les
Mac), passer `--device cpu` : ça marche, mais c'est plusieurs fois plus lent.

**Windows** (PowerShell) :

```powershell
uv venv --python 3.11 $HOME\itg-charter\demucs-venv
$env:VIRTUAL_ENV = "$HOME\itg-charter\demucs-venv"
# GPU NVIDIA : torch CUDA ; sans GPU, retirer la ligne --index-url et utiliser --device cpu
uv pip install torch torchaudio --index-url https://download.pytorch.org/whl/cu124
uv pip install demucs soundfile
setx ITG_CHARTER_PYTHON "$HOME\itg-charter\demucs-venv\Scripts\python.exe"
```

Pour un autre Python où Demucs est déjà installé : variable `ITG_CHARTER_PYTHON=/chemin/python`.

Le premier morceau télécharge les poids du modèle `htdemucs` (≈ 80 Mo). Les pistes séparées sont
ensuite mises en cache (`~/.cache/itg-charter/stems/`) : relancer un morceau ne refait pas Demucs.

### 5. Brancher le dossier de sortie sur le jeu (une fois)

itg-yt écrit dans `~/ITG-YouTube/`. Pour que le jeu le voie, on le relie au dossier `Songs`
d'ITGmania, où il apparaît comme le pack « YouTube ». Le dossier `Songs` est :

| système | installation portable | installation normale |
|---|---|---|
| Linux | `<dossier d'ITGmania>/Songs` | `~/.itgmania/Songs` |
| macOS | `<dossier d'ITGmania>/Songs` | `~/Library/Application Support/ITGmania/Songs` |
| Windows | `<dossier d'ITGmania>\Songs` | `%APPDATA%\ITGmania\Songs` |

```sh
# Linux / macOS (lien symbolique ; adapter le chemin de Songs)
mkdir -p ~/ITG-YouTube
ln -sfn ~/ITG-YouTube ~/.itgmania/Songs/YouTube
```

```bat
:: Windows (cmd, jonction : pas besoin d'être administrateur ; adapter le chemin de Songs)
mkdir %USERPROFILE%\ITG-YouTube
mklink /J "%APPDATA%\ITGmania\Songs\YouTube" "%USERPROFILE%\ITG-YouTube"
```

Sous Windows, `HOME` n'est en général pas défini, et itg-yt en a besoin pour trouver
`~/ITG-YouTube` et son cache. Le définir une fois : `setx HOME "%USERPROFILE%"` (puis ouvrir un
nouveau terminal), ou passer `-o` et `--cache` à chaque lancement.

Autre solution sur tous les systèmes : écrire directement dans le jeu avec
`-o <Songs>/YouTube`.

### Vérifier l'installation

```sh
itg-yt --help
yt-dlp --version
itg-yt --no-video "https://www.youtube.com/watch?v=..."   # premier essai rapide, sans le clip
```

La dernière ligne affichée est le chemin du `.sm` créé. Relancer ITGmania (ou recharger les
chansons) : la chanson est dans le pack « YouTube ».

## Utilisation

Une URL de vidéo avec `&list=…` (lecture depuis un mix ou une playlist) ne donne que cette
vidéo. Une URL de playlist donne toutes ses vidéos.

Le dossier `~/ITG-YouTube/<Titre>/` contient :

| fichier | contenu |
|---|---|
| `<Titre>.sm` | les charts, générés par [itg-charter](https://github.com/Alexis-benoist/itg-charter) (par défaut 5 charts de niveau 2, 4, 6, 8 et 10) |
| `<Titre>.ogg` | l'audio en OGG Vorbis q5 (≈160 kb/s, transparent à l'oreille, 3 à 4 Mo) |
| `<Titre>-bg.mp4` | le clip en fond animé, H.264 jusqu'à 1080p, 30 i/s, sans son |
| `bn.png`, `bg.png`, `jacket.png` | bannière 418×164, fond 1920×1080 et jaquette 512×512 tirés de la miniature |

## Options

Charts :
- `-p beginner` : niveaux 2, 3, 4, 5 ; `-p full` (défaut) : niveaux 2, 4, 6, 8, 10 ;
- `-m 2-5` ou `-m 1,3,6` : niveaux choisis sur l'échelle ITGmania 1-10 (5 au plus) ;
- `-d easy,hard` : à la place des niveaux, des cases de difficulté (beginner, easy, medium, hard,
  challenge ou `all`) avec la densité typique des charts humains de chaque case ;
- `-s 42` : seed (défaut 0 ; même vidéo + même seed ⇒ mêmes charts) ;
- `--no-stems` (sans Demucs, plus rapide), `--device cpu`.

Général : `-o DIR`, `--no-video`, `--title`, `--artist` (une seule vidéo), `--cache DIR`.

Plusieurs chansons :
- `--jobs 2` (défaut) : chansons téléchargées et encodées en même temps. Les charts (Demucs sur
  le GPU) passent une chanson à la fois, pendant que les suivantes se préparent ;
- une chanson en échec n'arrête pas les autres ; un résumé est affiché à la fin, les chemins des
  `.sm` réussis sont écrits sur la sortie standard, et le code de retour vaut 1 s'il y a eu un
  échec.

Fiabilité : yt-dlp relance lui-même les erreurs réseau (`--retries`, `--fragment-retries`,
`--extractor-retries`). Si un appel échoue quand même (plantage, extraction ratée), itg-yt le
relance jusqu'à `--yt-retries 3` fois, après 2 s, 4 s puis 8 s. Pour les métadonnées, l'appel
groupé est refait et les résultats sont fusionnés.

Vidéo de fond :
- `--video-height 720` : hauteur maximale (défaut 1080 ; jamais d'agrandissement) ;
- `--video-preset veryfast` : vitesse de x264 (défaut `medium`) ;
- `--video-crf 26` : qualité (plus bas = meilleure et plus grosse).

Mesuré sur « Yeah! » d'Usher (4 min 10, 12 cœurs, GTX 1650, réglages par défaut) : charts prêts
en 1 min 40, dossier complet en 5 min 20 ; OGG 4,7 Mo ; vidéo 1080p 115 Mo. L'encodage 1080p est
l'étape la plus longue : pour aller plus vite, `--video-height 720 --video-preset veryfast`.

Variables d'environnement : `ITG_YT_DLP` et `ITG_FFMPEG` (autre yt-dlp / ffmpeg),
`ITG_CHARTER_PYTHON` (Python avec Demucs).

## Fonctionnement

1. Métadonnées de **toutes les URL en un seul appel** `yt-dlp -j` (les challenges YouTube ne sont
   résolus qu'une fois ; doublons retirés). Titre et artiste viennent des champs musicaux de
   YouTube (`track`, `artists`) quand ils existent, sinon du titre nettoyé (« Artiste - Titre
   (Official Video) [4K] » → « Titre » / « Artiste »).
2. Téléchargement **en parallèle** de l'audio, de la vidéo (≤ 1080p) et de la miniature, dans un
   cache (`~/.cache/itg-charter/youtube/<id>/`) : relancer ne retélécharge rien.
3. Encodages ffmpeg **en parallèle** (audio, images, et vidéo en priorité basse), eux aussi mis en
   cache : relancer (autre seed, autres difficultés) ne réencode rien, et changer la qualité de la
   vidéo ne réencode que la vidéo.
4. Dès que l'OGG et les images sont prêts, itg-charter (compilé dans itg-yt) crée le dossier et
   génère les charts sur l'OGG, celui que le jeu lira, pendant que la vidéo s'encode encore.
5. itg-charter ajoute ensuite la vidéo de fond au `.sm` (`#BGCHANGES`, calée pour démarrer au
   début de l'audio).

## Dépannage

| symptôme | solution |
|---|---|
| `warning: no stems (Demucs python not found at …)` | pas bloquant (charts sur le mix seul) ; pour avoir les stems : étape 4 ou `ITG_CHARTER_PYTHON` ; pour le faire taire : `--no-stems` |
| erreur yt-dlp : `Sign in to confirm…`, `n challenge`, `Requested format is not available` | `uv tool upgrade yt-dlp`, vérifier qu'il a été installé avec `[default]` et que deno ou node est dans le `PATH` |
| `Unknown encoder 'libx264'` ou `'libvorbis'` | ffmpeg incomplet : prendre un build complet (étape 2) |
| `CUDA out of memory` | `--device cpu` (ou fermer ce qui occupe le GPU) |
| compilation : `implicit declaration of function 'strncasecmp'` | compiler depuis le clone (voir `.cargo/config.toml`) ou `export CFLAGS=-D_DEFAULT_SOURCE` |
| la chanson n'apparaît pas dans le jeu | vérifier le lien de l'étape 5, puis recharger les chansons ou relancer ITGmania |
| Windows : fichiers créés dans le dossier courant | `HOME` non défini : `setx HOME "%USERPROFILE%"` (étape 5) |

## Releases

Chaque push sur `main` publie automatiquement une release GitHub `vX.Y.<n° de build>` avec les
binaires Linux x86_64, macOS (arm64, x86_64) et Windows x86_64 (quand ils compilent), en
`.tar.gz`/`.zip` + sha256. Pousser un tag `vX.Y.Z` publie aussi une release sous ce nom.

## Licence

GPL-3.0, comme itg-charter.
