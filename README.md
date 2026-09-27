# itg-yt

Crée un dossier de chanson **ITGmania / In The Groove** complet à partir d'une vidéo YouTube :

```sh
itg-yt "https://www.youtube.com/watch?v=..."
```

Le dossier `~/.itgmania/Songs/YouTube/<Titre>/` contient :

| fichier | contenu |
|---|---|
| `<Titre>.sm` | les 5 difficultés (Beginner → Challenge), générées par [itg-charter](https://github.com/Alexis-benoist/itg-charter) |
| `<Titre>.ogg` | l'audio en OGG Vorbis q5 (≈160 kb/s, transparent à l'oreille, 3 à 4 Mo) |
| `<Titre>-bg.mp4` | le clip en fond animé, H.264 jusqu'à 1080p, 30 i/s, sans son |
| `bn.png`, `bg.png`, `jacket.png` | bannière 418×164, fond 1920×1080 et jaquette 512×512 tirés de la miniature |

Relancer ITGmania (ou recharger les chansons) pour la voir apparaître.

## Fonctionnement

1. Métadonnées avec `yt-dlp -J`. Le titre et l'artiste sont nettoyés (« Artiste - Titre (Official
   Video) [4K] » → « Titre » / « Artiste »).
2. Téléchargement **en parallèle** de l'audio, de la vidéo (≤ 1080p) et de la miniature, dans un
   cache (`~/.cache/itg-charter/youtube/<id>/`) : relancer ne retélécharge rien.
3. Encodages ffmpeg **en parallèle** : audio, images, et vidéo (en priorité basse).
4. Dès que l'OGG est prêt, `itg-charter gen` génère les charts sur ce fichier, celui que le jeu lira,
   pendant que la vidéo s'encode encore.
5. Le `.sm` est complété avec la bannière, le fond, la jaquette et la vidéo
   (`#BGCHANGES`, calée pour démarrer au début de l'audio).

## Options

`-d easy,hard` (défaut : toutes), `-s 42` (seed, défaut 0), `-o DIR`, `--no-video`, `--no-stems`
(sans Demucs, plus rapide), `--device cpu`, `--title`, `--artist`, `--cache DIR`.

Vidéo de fond :
- `--video-height 720` : hauteur maximale (défaut 1080 ; jamais d'agrandissement) ;
- `--video-preset veryfast` : vitesse de x264 (défaut `medium`) ;
- `--video-crf 26` : qualité (plus bas = meilleure et plus grosse).

Ordre de grandeur mesuré pour un clip de 3 min 33 (i7 12 cœurs, GTX 1650) : charts prêts en
2 min ; OGG 4 Mo ; vidéo 1080p ≈ 70 Mo. L'encodage 1080p est l'étape la plus longue : pour aller
plus vite, utiliser `--video-height 720 --video-preset veryfast`.

## Prérequis

- [itg-charter](https://github.com/Alexis-benoist/itg-charter) compilé : dans le PATH,
  dans `~/itg-charter/target/release/`, ou désigné par `ITG_CHARTER_BIN`.
- yt-dlp avec ses scripts de résolution YouTube et un runtime JavaScript (node ou deno) :
  `uv tool install "yt-dlp[default]"`.
- ffmpeg avec libvorbis et libx264.

Les outils peuvent être remplacés par `ITG_YT_DLP`, `ITG_FFMPEG` et `ITG_CHARTER_BIN`.

## Licence

GPL-3.0, comme itg-charter.
