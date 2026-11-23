## prune — deux racines de modèles en journaux

`koharu-batch prune` classe les deux racines du store, pas seulement celle que
0.92 écrit :

- `store/huggingface` (ce que 0.92 télécharge) — gardé au **commit**, comme avant ;
- `store/hugging-face` (ce que le fork téléchargeait avant le renommage upstream
  de Hugging Face) — gardé au **dépôt** : un dépôt qu'un catalogue pointe encore
  reste entier, quel que soit le révision que le fork y a laissée.

Ce n'est pas une compatibilité. `prune` lit seulement les pins 0.92 et leurs
dépôts/downloads. Le fork conservé sert à ne pas supprimer un dépôt encore
proposé ; on ne propose jamais un dépôt `hugging-face` en l'état.

Rapport : deux blocs, `périmés` puis `réutilisables`, avec leur taille ; rien
d'invisible dans les ~50 GiB de `hugging-face` qui correspond à un pin 0.92.

### Cas `baberu-ocr`

Pin 0.92 : `genshiai-daichi--baberu-ocr` à `a0e3d4b1…` (`huggingface`). Le store
contient `genshiai-daichi--baberu-ocr` à `d9cc131…` sous `hugging-face`. Même
dépôt, autre révision : le dépôt est gardé au nom du pin, mais le télécharge
fork ne correspond pas au commit 0.92 — adoption = re-télécharge d'environ 1 GiB,
choix conservateur assumé.

Décision committée sur la branche `port/upstream-0.92`. Le rapport affiche les
périmés puis les réutilisables séparément ; les réutilisables sont dérivés des
deux racines en même temps que les pins, sans récrire de catalogue.
