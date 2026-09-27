# itg-yt

Crée un dossier de chanson **ITGmania / In The Groove** complet à partir d'une vidéo YouTube :

```sh
itg-yt "https://www.youtube.com/watch?v=..."
itg-yt URL1 URL2 URL3                 # plusieurs chansons
itg-yt "https://www.youtube.com/playlist?list=..."   # toute une playlist
itg-yt -a mes-chansons.txt            # une URL par ligne (# = commentaire, - = stdin)
```

Une URL de vidéo avec `&list=…` (lecture depuis un mix ou une playlist) ne donne que cette
vidéo. Une URL de playlist donne toutes ses vidéos.

Le dossier `~/ITG-YouTube/<Titre>/` contient :

| fichier | contenu |
|---|---|
| `<Titre>.sm` | les 5 difficultés (Beginner → Challenge), générées par [itg-charter](https://github.com/Alexis-benoist/itg-charter) |
| `<Titre>.ogg` | l'audio en OGG Vorbis q5 (≈160 kb/s, transparent à l'oreille, 3 à 4 Mo) |
| `<Titre>-bg.mp4` | le clip en fond animé, H.264 jusqu'à 1080p, 30 i/s, sans son |
| `bn.png`, `bg.png`, `jacket.png` | bannière 418×164, fond 1920×1080 et jaquette 512×512 tirés de la miniature |

Relancer ITGmania (ou recharger les chansons) pour la voir apparaître dans le pack « YouTube ».

### Brancher le dossier sur le jeu (une fois)

`~/ITG-YouTube` est relié au jeu par un lien symbolique dans son dossier `Songs` :

```sh
ln -sfn ~/ITG-YouTube ~/Downloads/ITGmania-1.1.0-Linux-no-songs/itgmania/Songs/YouTube
```

(Adapter le chemin de l'installation d'ITGmania ; `-o DIR` change le dossier de sortie.)

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
4. Dès que l'OGG et les images sont prêts, `itg-charter gen` crée le dossier et génère les charts sur
   l'OGG, celui que le jeu lira, pendant que la vidéo s'encode encore.
5. `itg-charter decorate` ajoute ensuite la vidéo de fond (`#BGCHANGES`, calée pour démarrer au
   début de l'audio).

## Options

`-d easy,hard` (défaut : toutes), `-s 42` (seed, défaut 0), `-o DIR`, `--no-video`, `--no-stems`
(sans Demucs, plus rapide), `--device cpu`, `--title`, `--artist` (une seule vidéo), `--cache DIR`.

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

## Releases

Pousser un tag `vX.Y.Z` publie une release GitHub avec le binaire Linux (`.tar.gz` + sha256).

## Prérequis

- [itg-charter](https://github.com/Alexis-benoist/itg-charter) compilé, avec les sous-commandes
  `gen --banner …` et `decorate` : dans le PATH, dans `~/itg-charter/target/release/`, ou désigné
  par `ITG_CHARTER_BIN`.
- yt-dlp avec ses scripts de résolution YouTube et un runtime JavaScript (node ou deno) :
  `uv tool install "yt-dlp[default]"`.
- ffmpeg avec libvorbis et libx264.

Les outils peuvent être remplacés par `ITG_YT_DLP`, `ITG_FFMPEG` et `ITG_CHARTER_BIN`.

## Licence

GPL-3.0, comme itg-charter.
