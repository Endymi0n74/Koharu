Ménage code + branches (26/11/2026)
===================================
Décisions : prune du store puis suppression du legacy 60 Go, suppression des 33 branches distantes, target et bench-profile nettoyés, dist conservé, doc commitée, HANDOFF/CLAUDE/.agents conservés.

Vérifié :
- 12 locales réécrites par PowerShell (indentation 4 espaces, échappements \u003c/\u003e/\u0027) remises au format du projet par oxfmt ; contenu identique (hash JSON comparé avant/après), diff vs 059238f0^ réduit aux ajouts batch du fork.
- packages/bridge supprimé (wasm généré, plus référencé).
- Commits locaux aaf962eb (docs fork fr/ja/zh) et a268d646 (style locales), sans Co-Authored-By ; non poussés.
- Fichiers générés supprimés : node_modules/.old_modules-*, packages/koharu/.next, packages/koharu/out, tsconfig.tsbuildinfo, next-env.d.ts, crates/koharu/binaries, crates/koharu/gen, .cloud-attachments, .freebuff.
- koharu-batch prune --delete : 9 anciennes versions CUDA, 1,9 Gio.
- store/hugging-face supprimé (60 Go) : le runtime ne lit que huggingface/<kind>/<repo>/snapshots/<commit> (crates/koharu-runtime/src/huggingface.rs).
- target/ supprimé (59 Go) ; bench-profile/ déplacé vers D:\codex\koharu-data\bench-profile-2026-11-26 (307 Mo) ; dist/ conservé.
- git fetch --prune upstream (17 refs disparues), tag local merge-upstream-main supprimé, main suit désormais origin/main.
- 33 branches supprimées sur origin (17 miroirs upstream, 14 dependabot, port/upstream-0.92, qwen-image-2.1) ; origin ne garde que main. Bundle de sauvegarde de qwen-image-2.1 dans D:\codex\koharu-data\backups (97 Mo).

Non exercé :
- aucun build complet après suppression de target (aucun code Rust/TS modifié) ; prochaine compilation ~5-6 min.
- modèles du legacy absents du root courant : re-téléchargés au premier usage.
- push de main non fait (2 commits locaux).
- oxlint racine était cassé avant le ménage ; bun install non relancé.

Bundle installé déplacé hors du dépôt (25/11/2026)
=====================================================
Le bundle installé (koharu.exe, DLL CEF, paks, locales, uninstall.exe) a été déplacé de la racine du dépôt vers D:\Codex\KoharuBatch.
Les raccourcis Bureau et Menu Démarrer ont été mis à jour pour pointer vers D:\Codex\KoharuBatch\koharu.exe avec workdir D:\Codex\KoharuBatch.
La clé registry de désinstallation HCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\koharu a été mise à jour pour rester cohérente (InstallLocation, UninstallString, DisplayIcon).
.git/info/exclude a été nettoyé du bloc d'exclusion du bundle.
Le commit local 30b5d6d1 documente ce déplacement dans HANDOFF.md sans Co-Authored-By.

Vérification de clôture (raccourci bureau, 25/11/2026)
---------------------------------------------------------
Ce qui a été vérifié :
- déplacement du bundle vers D:\Codex\KoharuBatch
- mise à jour des raccourcis (Bureau et Menu Démarrer) avec relire après écriture
- lancement de l'app via le raccourci bureau (fenêtre KoharuBatch ouverte puis fermée)
- point registry cohérent avec le nouveau chemin
Ce qui reste non exercé :
- désinstallation réelle via uninstall.exe non lancée

Résumé vérifié / non vérifié — déplacement du bundle (25/11/2026)
------------------------------------------------------------------
Vérifié :
- bundle déplacé vers D:\Codex\KoharuBatch (koharu.exe, DLL CEF, paks, locales, uninstall.exe, bootstrap.exe, bootstrapc.exe, CREDITS.html)
- raccourcis Bureau et Menu Démarrer mis à jour et relus après écriture
- lancement via raccourci bureau (fenêtre ouverte puis fermée)
- clé registry Uninstall cohérente avec le nouveau chemin
- .git/info/exclude sans le bloc bundle, git status propre
- build release reconstruit depuis la source et app lancée depuis target/release avant le déplacement
Non vérifié dans ce tour :
- désinstallation réelle via uninstall.exe non lancée
- raccourci du menu Démarrer testé de manière symétrique au bureau
- confirmation que la prochaine distribution portable/installeur n'attend plus un runtime à la racine du dépôt

Temp — scripts temporaires sous Windows (25/11/2026)
----------------------------------------------------
Décision :
- exclude.bak conservé dans %TEMP% sans le déplacer : sauvegarde hors dépôt, non suivie, issue d'une opération précédente ; la version actuelle de .git/info/exclude est déjà sans le bloc bundle, donc la sauvegarde n'a plus de valeur opérationnelle immédiate et peut être supprimée sans impact sur le dépôt. Conservé pour l'instant car non destructif et hors du périmètre du dépôt.
- les scripts temporaires récents (%TEMP%/*.ps1, *.bat, *.cmd) ont été scannés pour des restes de commandes avant nettoyage : aucun .ps1/.bat/.cmd présent dans %TEMP% après le nettoyage ciblé déjà effectué cette session ; les fichiers .tmp et journaux Windows présents ne sont pas des scripts exécutables de cette session.
Suppression des scripts temporaires sous Windows (%TEMP%) — 25/11/2026
- script de lecture des raccourcis (read-lnk.ps1) supprimé
- script de lancement du raccourci bureau (launch-koharu-lnk.bat) supprimé
- script d'ajout au HANDOFF.md (append-handoff.ps1) supprimé
- dossier temporaire du lancement codebuff-process-wjzy4p supprimé
- anciens répertoires temporaires codebuff-process-* de plus de 48 h supprimés (liste non exhaustive conservée dans un premier temps)
- pas de .ps1/.bat/.cmd restant dans %TEMP% après nettoyage (vérifié par recherche find)
- désormais aucun script créé dans cette session ne semble rester dans %TEMP%

Notes de la session (25/11/2026)
--------------------------------
- Ménage limité aux fichiers ignorés/non suivis jetables ; caches target/debug, target/release, .fingerprint conservés.
- Bundle déplacé vers D:\Codex\KoharuBatch (dossier choisi comme « dédié hors du dépôt »).
- bootstrap.exe, bootstrapc.exe et CREDITS.html déplacés avec le bundle car visés par les mêmes règles d'exclusion à retirer.
- Registre de désinstallation mis à jour pour rester cohérent (pas de suppression de clé).
- Aucun commit créé pendant la phase de déplacement : seul HANDOFF.md est modifié (non commité puis commité dans la foulée).
- Raccourcis mis à jour (cible/workdir/icône D:\Codex\KoharuBatch\koharu.exe) : %APPDATA%\Microsoft\Windows\Start Menu\Programs\koharu.lnk et %USERPROFILE%\Desktop\koharu.lnk — relus après écriture, corrects.
- Bloc « Application Koharu installee dans le dossier du depot » retiré de .git/info/exclude (suppression par sed, sauvegarde /tmp/exclude.bak) ; git status propre après retrait.
- Commit local créé pour HANDOFF.md avec message `Journal : bundle installé déplacé hors du dépôt (25/11)` et sans Co-Authored-By.
- Nettoyage des scripts temporaires dans %TEMP% ciblé : les fichiers .ps1/.bat/.cmd créés dans cette session, plus le dossier codebuff-process-wjzy4p.

Nettoyage effectué avant ce tour (fin de la session précédente)
--------------------------------------------------------------
- Ménage racine : suppression de ipc.exe/.pdb/.d, koharu.pdb, koharu-batch.d, koharu.d, koharu_batch.pdb, libkoharu.rlib/.d, libkoharu_desktop.rlib/.d, libcef.lib, llguidance.h, CMakeLists.txt, archive.json, _up_/, .cargo-*-lock, debug.log, launch-release.log.
- Nettoyage target/ (fichiers maxdepth 1 : logs, scripts, dumps, copies de bundles portable-test/, zip-check/, cli-check/, tmp/, restes CEF dupliqués).
- dist/ : suppression de l'ancien paquet KoharuBatch-0.93.0-fork-windows-x64-portable/ et .zip ; paquet 0.93.0 + SHA256SUMS conservés.
- Build release vérifié : cargo build --release -p koharu → exit 0, Finished release profile in 5m 23s, avec cef-dll-sys reconfiguré et recompilé depuis la source (preuve que CMakeLists.txt, libcef.lib, llguidance.h racine étaient inutiles).
- App lancée depuis target/release : fenêtre « KoharuBatch », Responding: True, 5 sous-processus CEF, log CEF's frames: shared textures on NVIDIA GeForce RTX 3070 (vulkan).
- Bundle déplacé vers D:\Codex\KoharuBatch : 19 fichiers + locales\ (220 .pak) ; racine sans aucun fichier d'app.
- Raccourcis relus après écriture : cible D:\Codex\KoharuBatch\koharu.exe, workdir D:\Codex\KoharuBatch, icône D:\Codex\KoharuBatch\koharu.exe,0 ; lus via WScript.Shell dans un script PS1 puis supprimé.
- HCKU\Software\Microsoft\Windows\CurrentVersion\Uninstall\koharu : InstallLocation="D:\Codex\KoharuBatch", UninstallString="D:\Codex\KoharuBatch\uninstall.exe", DisplayIcon="D:\Codex\KoharuBatch\koharu.exe".
- .git/info/exclude : bloc d'exclusion du bundle supprimé ; git status propre.
- App relancée depuis D:\Codex\KoharuBatch : Path confirmé par Get-Process, fenêtre « KoharuBatch » répondante, 5 processus CEF ; instance fermée ensuite (aucun koharu restant).
- HANDOFF.md : nouvelle section en tête « Bundle installé déplacé hors du dépôt (25/11) » ; puis ajout d'une sous-section « Vérification de clôture (raccourci bureau, 25/11) » listant déplacement, raccourcis, lancement par raccourci bureau, point registry, et le point non exercé : désinstallation réelle (uninstall.exe) non lancée.
- Commit local 30b5d6d1 sur port/upstream-0.92 : 1 file changed, 26 insertions, auteur/commiteur Endymion <stephane.rivoire74@gmail.com>, pas de Co-Authored-By.
- Lancement via raccourci bureau (via start "" "%USERPROFILE%\Desktop\koharu.lnk") : fenêtre « KoharuBatch » ouverte puis fermée.
- Nettoyage %TEMP% : supprimé read-lnk.ps1, launch-koharu-lnk.bat, append-handoff.ps1, et codebuff-process-wjzy4p (stdout/stderr de ce lancement). Nettoyage des répertoires temporaires codebuff-process-* de plus de 48 h.

Dernières vérifications (fin de session précédente)
----------------------------------------------------
- confirmer que la prochaine distribution portable/installeur n'attend plus un runtime à la racine du dépôt : non vérifié dans ce tour
- retester le raccourci du menu Démarrer de manière symétrique au bureau : non vérifié dans ce tour
- confirmer que le lancement via raccourci bureau est stable : vérifié pour le lancement (fenêtre ouverte puis fermée)
- valider l'uninstall réelle via uninstall.exe : non exercé (consigné comme non exercé)

État des vérifications (avant ce tour)
---------------------------------------
### Vérifié
- déplacement du bundle hors du dépôt
- mise à jour des raccourcis (Bureau et Menu Démarrer)
- lancement via raccourci bureau
### Non exercé
- désinstallation réelle via uninstall.exe
- retest du raccourci du menu Démarrer de manière symétrique
- vérification que la prochaine distribution portable/installeur n'attend plus un runtime à la racine du dépôt