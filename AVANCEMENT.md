# Avancement — Morph-Format v1

Mis à jour le 2026-11-08.

**État : v1 terminée et empaquetée (installeur seul, le .zip portable a été retiré le 2026-11-08).** Pour produire l'installeur : `powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1` (sortie dans `dist\`).

Terminées : 12 / 12 · En cours : 0

| # | Tâche | Dépend de | État |
|---|---|---|---|
| T1 | Base du projet : workspace Cargo, licences MIT/Apache, README, API commune du moteur | — | Terminée (`c8aeacf`) |
| T2 | Moteur image : JPG, PNG, WebP, GIF, BMP, qualité, taille, EXIF | T1 | Terminée (`fbe428c`) |
| T3 | Lecture HEIC (photos iPhone) via libheif | T2 | Terminée (`837f657`) |
| T4 | Moteur vidéo : lecture MP4/MOV/MKV/WebM, sortie WebM VP9/AV1, MKV | T1 | Terminée (`c80c491`) |
| T5 | Sortie MP4 H.264 (encodeurs du système) et GIF animé | T4 | Terminée (`5206586`) |
| T6 | Interface egui : glisser-déposer, file, progression, réglages | T1 | Terminée (`c1f4130`) |
| T7 | Licences des dépendances : cargo-deny, cargo-about, doc juridique | T1 | Terminée (`f3c3578`) |
| T8 | Finitions : masquer H.264 s'il est indisponible, liens licences dans le README | T5 | Terminée (`6708d0d`) |
| T9 | Installeur Windows (cargo-packager, NSIS) + .zip portable, icône, sans console | — | Terminée (`b234b92`) |
| T10 | Rattraper H.264 VAAPI (Linux), commité après l'intégration de T5 (`f08f069`) | — | Terminée (`529f527`) |
| T11 | README réécrit en anglais, centré sur l'installation | — | Terminée (`f8efb63`) |
| T12 | Refonte de l'interface selon le rapport du designer (anglais, hiérarchie, progression globale, états, couleurs AA) | — | Terminée (`474109e`) |

## Vérification finale (2026-11-06)

- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace` : OK (build par défaut).
- `cargo test -p morph-core --features heic,video` : OK (HEIC, WebM, MKV, MP4 H.264 via Media Foundation, GIF).
- `cargo build -p morph-app --features heic,video` : OK.
- FFmpeg compilé en LGPL (`--disable-libx264 --disable-libx265`, sans `--enable-gpl` ni `--enable-nonfree`).

## Reste à faire (toi)

- Lancer l'appli et la tester à la main : personne ne l'a encore ouverte.
- Tester H.264 sous macOS et Linux (VAAPI non testé).
- Vérifier les marques déposées pour le nom avant de publier.
- Pousser sur GitHub quand tu es prêt.

## Historique

- 2026-11-05 — Commit initial `a943239`, plan et tâches créés, mis en pause avant T1.
- 2026-11-06 — T1 lancée (worker Ava, 2ᵉ et dernier essai).
- 2026-11-06 — Commande de vérification corrigée pour PowerShell 5.1 (`cmd /c "…"`).
- 2026-11-06 — T1 intégrée sur `main` (`c8aeacf`). T2, T4 et T6 lancées.
- 2026-11-06 — T6 (interface egui) intégrée sur `main` (`c1f4130`). T7 lancée.
- 2026-11-06 — T2 (moteur image) intégrée sur `main` (`fbe428c`). T3 lancée.
- 2026-11-06 — T3 (lecture HEIC) intégrée sur `main` (`837f657`).
- 2026-11-06 — T7 (licences des dépendances) intégrée sur `main` (`f3c3578`).
- 2026-11-06 — T4 (moteur vidéo) intégrée sur `main` (`c80c491`). T5 lancée, T8 ajoutée.
- 2026-11-06 — T5 (MP4 H.264 et GIF) intégrée sur `main` (`5206586`). T8 lancée.
- 2026-11-06 — T8 (finitions) intégrée sur `main` (`6708d0d`). Vérification finale OK.
- 2026-11-06 — T9 (installeur + version portable, sans signature) ajoutée et lancée.
- 2026-11-06 — T9 intégrée sur `main` (`b234b92`) : installeur NSIS par utilisateur + .zip portable, 11 DLL, licences LGPL, icône, sans console.
- 2026-11-06 — Nettoyage : ancien worktree T1, morph-test, vcpkg-jude et le vcpkg temporaire de Finn supprimés (copies dans `morph-deps`). Découverte : le commit VAAPI `f08f069` n'était pas sur `main` → T10 lancée.
- 2026-11-06 — T10 intégrée sur `main` (`529f527`), branche de Cleo supprimée. Il ne reste que `main`.
- 2026-11-07 — T11 (README en anglais) intégrée sur `main` (`f8efb63`).
- 2026-11-08 — Poussé sur GitHub. Historique réécrit avec l'adresse noreply GitHub (identifiants de commit mis à jour ici).
- 2026-11-08 — Revue de l'interface par un agent designer (16 problèmes). T12 (refonte) lancée.
- 2026-11-08 — T12 intégrée sur `main` (`474109e`). Build complet (HEIC + vidéo), installeur et .zip régénérés dans `dist\`.
- 2026-11-08 — .zip portable supprimé (script, README, docs/packaging.md). Ajouts non commités : curseur Scale corrigé, aide « ? » (Metadata, Scale, Resolution), mode Resolution (2160p→360p) à la place de Fit within.
- 2026-11-08 — Bug corrigé : MP4 indisponible dans l'appli (Media Foundation refusait de s'ouvrir sur le fil COM STA de l'interface ; la détection H.264 tourne maintenant sur son propre fil). Test de non-régression ajouté. Non commité.
- 2026-11-08 — Mode Scale retiré de l'interface : Resize = Original ou Resolution. Non commité.
- 2026-11-08 — Resize simplifié en une liste « Resolution » : Keep original (défaut), 4K, 1440p, 1080p, 720p, 480p, 360p. Non commité.
