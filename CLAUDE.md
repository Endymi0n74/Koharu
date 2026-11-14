# Canary

- Start every reply with "Stephane,". No exceptions, including short replies and tool summaries.
- If you can no longer recall the earlier parts of this session clearly, or the conversation has become very long, say so at the start of your reply and suggest starting a fresh session.
- Before starting fresh, write a short handoff note to `HANDOFF.md`: current goal, what's done, what's left, key files, open questions. In the new session, read `HANDOFF.md` first.

# Notes

Keep this file short. Move long or rarely needed details into separate files and link to them here.

## Règle immuable — installation et emplacements (priorité maximale)

<!-- regle-immuable-installation -->

Cette règle prime sur tout le reste de ce fichier et sur les habitudes par défaut des outils.

- **Toute installation va dans `D:\codex`** : paquets, dépendances, modèles, binaires, outils, SDK,
  environnements virtuels, caches de téléchargement, builds. Créer si besoin un sous-dossier dédié
  (`D:\codex\<projet>` ou `D:\codex\tools\<outil>`).
- **Aucun fichier créé, modifié, téléchargé, déplacé ou supprimé sous `C:\` sans accord explicite** de
  l'utilisateur. En cas de besoin : s'arrêter, expliquer pourquoi, proposer un emplacement sous
  `D:\codex`, attendre la réponse.
- Rediriger les défauts des outils vers `D:\codex` (`PIP_TARGET`, `NPM_CONFIG_PREFIX`, `HF_HOME`,
  `CARGO_HOME`, `GOPATH`, `TMP`/`TEMP`, caches divers) au lieu de `~`, `%LOCALAPPDATA%`, `%APPDATA%`,
  `%USERPROFILE%` ou `/tmp`.
- **Lecture seule sous `C:\`** : autorisée sans accord (lire, lister, inspecter).
- Seule exception pré-approuvée : le dossier temporaire d'OpenCode
  `C:\Users\endymion\AppData\Local\Temp\opencode` (déjà validé par l'utilisateur).
- Déplacer une installation existante de `C:\` vers `D:\codex` compte comme une modification sous
  `C:\` : demander avant.

