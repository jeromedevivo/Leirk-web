# Leirk — revue de sécurité (2026-11-18)

Périmètre : `src/*.rs`, `ui/index.html`, `ui/extra.js`, `ui/defaults.json`, `build.rs`.
Attaquants considérés : une page web malveillante (y compris une iframe publicitaire), un fichier piégé dans le dossier de synchronisation, un attaquant réseau (Wi-Fi public), et un programme local placé sur le PATH.

## Ce qui tenait déjà
- **Les pages ne pilotent pas le navigateur** : le canal IPC des pages n'accepte que `F2`, `F6`, `g:<geste>` et `c:<classes>`. Toutes les commandes sensibles (Docker, import, réglages, registre, dossiers) passent par la WebView d'interface, qui ne charge aucune page web.
- **Interface sans injection** : chaque texte venant d'une page (titre, URL, favori, historique, cookies…) passe par `esc()` ou `textContent`. Les données envoyées de Rust à l'interface sont sérialisées en JSON.
- **SQL** : toutes les requêtes sont paramétrées. La recherche échappe `%` et `_`, et la seule valeur insérée dans le texte SQL est un entier (`LIMIT`).
- **Commandes système** : aucun shell. Les arguments sont passés en tableau, les identifiants Docker sont vérifiés (alphanumériques), et les actions Docker sont limitées à start, stop et restart.
- **Onglets privés** : jamais écrits sur le disque (session, historique, icônes, synchro).
- **Regex des empreintes** : le moteur `regex` de Rust s'exécute en temps linéaire, avec une limite de taille, ce qui écarte tout déni de service par expression régulière.

## Failles trouvées et corrigées

| # | Gravité | Problème | Exploitation | Correctif |
|---|---|---|---|---|
| 1 | Élevée (vie privée) | Le journal `leirk.log` enregistrait les arguments des commandes : adresses visitées, recherches tapées (**y compris en navigation privée**), valeurs de cookies modifiées. | Quiconque lit le dossier `%LOCALAPPDATA%\Leirk` (sauvegarde, malware, autre personne sur le PC) retrouvait l'historique privé. | Seul le nom de la commande est journalisé, jamais les arguments. Rotation du fichier à 1 Mo. |
| 2 | Moyenne | Une page pouvait ouvrir un **nouvel onglet vers une adresse non web** (`file:///C:/…`, `data:`, `javascript:`) via `window.open` ou `target=_blank`. Leirk relançait la navigation lui-même, ce qui contournait le blocage de Chromium. | Ouvrir des fichiers locaux, ou un document `data:` d'hameçonnage en plein écran, depuis n'importe quel site. | Seules les adresses `http(s)://` et `about:blank` sont acceptées pour les nouveaux onglets ouverts par une page. |
| 3 | Moyenne | **Gestes souris forgés** : n'importe quelle page, y compris un onglet en arrière-plan ou une iframe, pouvait envoyer `g:DR` (fermer) ou `g:D` (nouvel onglet) en boucle. | Fermer l'onglet actif d'un autre site, ou inonder le navigateur d'onglets. | Les gestes ne sont acceptés que depuis l'onglet affiché, au plus un toutes les 300 ms. |
| 4 | Moyenne | **Fichier de synchro piégé** : favoris, moteurs et onglets distants étaient acceptés tels quels, y compris un moteur `javascript:…%s`. | Toute personne ayant accès en écriture au dossier cloud (dossier partagé, compte compromis) faisait exécuter du code dans vos pages à la recherche suivante. | Les documents distants sont filtrés : seules les adresses web sont gardées. L'ajout manuel d'un moteur exige aussi une adresse `http(s)` contenant `%s`. |
| 5 | Moyenne | Les **listes de filtres personnalisées** acceptaient `http://`, et curl pouvait suivre une redirection vers un autre protocole. Or une liste peut injecter des scriptlets dans les pages. | Sur un Wi-Fi public, un attaquant remplace la liste et injecte des scripts sur tous les sites. | Seules les listes en HTTPS sont acceptées. curl est forcé en `--proto =https --proto-redir =https`. |
| 6 | Faible à moyenne | Une page pouvait fournir une **icône `file://` ou UNC** (`\\serveur\partage`), affichée ensuite par l'interface. | Fuite potentielle du hash NTLM Windows vers un serveur SMB (Chromium bloque normalement ces chargements ; correctif ajouté en défense en profondeur). | Seules les icônes `http(s)` et `data:image/` sont acceptées. **CSP stricte** sur l'interface : aucun script externe, aucune connexion, images `https/http/data` uniquement. |
| 7 | Faible | Outils système appelés par leur nom (`curl.exe`, `reg.exe`, `netstat`, `tasklist`, `explorer.exe`). | Un `curl.exe` piégé placé plus tôt dans le PATH serait exécuté. | Chemins absolus vers `%SystemRoot%\System32`. |
| 8 | Faible | Les copies temporaires des bases d'autres navigateurs (historique, icônes) **restaient dans `%TEMP%`** après l'import. | Votre historique Brave/Chrome restait lisible en clair dans le dossier temporaire. | Les copies sont supprimées dès qu'elles ne servent plus (`TempDb` avec `Drop`). |
| 9 | Faible | Nom de fichier du cache des listes : une URL pouvait produire le nom `..`. | Écriture de fichier en dehors du dossier `lists`, en théorie. | Préfixe `list-` systématique. Test ajouté. |
| 10 | Faible | Nom de conteneur utilisé tel quel comme nom de profil WebView2. | Caractères inattendus dans un nom de dossier de profil. | Restreint à `[A-Za-z0-9_-]`, 32 caractères au maximum. |
| 11 | Faible | Ligne de commande (lancement comme navigateur par défaut) : tout argument était ouvert, même `-…` ou `javascript:`. | Un raccourci ou un lien piégé fait ouvrir une adresse non web. | Seuls un fichier existant ou une adresse `http(s)` sont acceptés. Les options commençant par `-` sont ignorées. |

Tests ajoutés : `only_web_addresses_from_untrusted_sources`, `list_cache_names_are_file_safe`, et le refus des listes en HTTP dans `list_selection_follows_prefs`.

## Risques connus, acceptés ou à traiter plus tard
- **Retour en HTTP après échec du HTTPS** (comportement standard de Brave) : un attaquant réseau qui bloque le port 443 obtient la version HTTP. Un mode strict pourra être ajouté.
- **Ressources Brave (`resources.json`) et listes uBlock** : elles sont chargées depuis GitHub en HTTPS, sans signature vérifiée. On fait confiance aux mêmes sources que Brave.
- **Réglages synchronisés** : un fichier de synchro piégé peut encore modifier des réglages comme les filtres personnels. Le dossier de synchro doit rester privé.
- **Panneaux web** (Teams, Spotify…) : ils ne passent pas encore par les Shields.
- **Applications ancrées** (expérimental) : si Leirk plante pendant qu'une application est ancrée, celle-ci se ferme avec lui.
- **Dépendances** : `cargo audit` n'est pas installé, donc la base RustSec n'a pas été consultée.
