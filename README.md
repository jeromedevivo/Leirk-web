# Leirk

Navigateur léger en Rust : l'ergonomie et les Shields de Brave, les piles d'onglets, la mosaïque et les commandes rapides de Vivaldi. Il est conçu pour que 50 onglets ouverts ne coûtent presque rien en mémoire.

## Lancer

```powershell
cargo build --release
.\target\release\leirk.exe            # ou : leirk.exe https://exemple.fr
```

Prérequis : Windows 10/11 avec le runtime WebView2 (déjà présent sur Windows 11), Rust stable et MSVC.
Données : `%LOCALAPPDATA%\Leirk\` (session, profil, historique SQLite, listes de filtres, données WebView2).

## Pourquoi c'est léger

- **Un onglet est une ligne de données, pas un processus.** Seuls les onglets visibles et les N derniers utilisés possèdent une WebView2 (4 par défaut, réglable). Les autres sont déchargés et se rechargent quand on y revient.
- Un onglet caché passe en mémoire basse (`MemoryUsageTargetLevel=Low`) et est suspendu (`TrySuspend`), sauf s'il joue du son.
- Après 15 minutes d'inactivité, il est déchargé (réglable, ou « Jamais »).
- Un nouvel onglet ne crée aucune WebView : la page nouvel onglet est dessinée par l'interface.
- La restauration de session ne charge que l'onglet actif.
- L'exécutable fait 5 Mo ; le processus Leirk pèse environ 15 Mo, moteur Shields compris.

Mesures sur cette machine (Mo privés, Leirk + ses processus WebView2) :

| Scénario | Leirk | Brave (profil vierge, mêmes sites) |
|---|---|---|
| 12 onglets restaurés au démarrage | **331 Mo**, 8 processus | — |
| 51 onglets restaurés au démarrage | **340 Mo**, 9 processus | — |
| 51 onglets, 2 en mosaïque | **383 Mo**, 9 processus | — |
| 12 sites ouverts un par un (4 vivants) | ~1 550–1 700 Mo, 13 processus | ~1 975 Mo, 25 processus |

Sur des sites lourds, l'essentiel restant est le processus GPU de WebView2. Les onglets vidéo qui jouent du son (YouTube, Twitch) ne sont volontairement jamais déchargés.

## Fonctionnalités

**Brave**
- Shields : le moteur `adblock-rust` de Brave avec son catalogue de listes, soit 30 listes par défaut : uBlock Origin 2020→2026, badware, quick-fixes, unbreak, EasyList, EasyPrivacy, URLhaus, listes Brave, bannières de cookies et AdGuard Français. Il gère aussi :
  - les **scriptlets** et les **redirections neutres** issus des ressources Brave ;
  - le masquage **générique** (classes et identifiants) et le masquage propre à chaque site, injecté dès la création du document ;
  - les service workers.

  Trois niveaux : Standard, Agressif (avec AdGuard Base, Tracking et URL Tracking) ou Désactivé. On peut ajouter des listes et des filtres personnels. Score sur adblock-tester.com : **100/100**.
- Par site : interrupteur Shields et blocage du JavaScript.
- Passage en HTTPS (retour en HTTP si le site n'a pas de version sécurisée), retrait des paramètres de pistage (utm, fbclid, gclid…), Global Privacy Control, blocage des pings, protection anti-pistage du moteur (4 niveaux).
- Onglets privés (InPrivate, Ctrl+Maj+N) avec un moteur de recherche à part.
- Effacement des données (période et types au choix) et effacement automatique à la fermeture.

**Vivaldi**
- Piles d'onglets, mosaïque de 2 à 4 onglets, commandes rapides (F2 / Ctrl+K) avec calculatrice.
- Gestes souris (← → ↓ ↓→ ↑↓ ↓←) et gestes à bascule.
- Options d'onglets : position des nouveaux onglets, onglet activé à la fermeture, ouverture des liens en arrière-plan, Ctrl+Tab dans l'ordre d'utilisation, double-clic pour fermer, molette pour changer d'onglet, atténuation des onglets déchargés.

**Interface**
- Barre de favoris : toujours, sur les nouveaux onglets seulement, ou jamais (Ctrl+Maj+B). Elle gère les dossiers, le débordement », le clic droit (ouvrir, arrière-plan, privé, modifier, supprimer) et le clic molette.
- Fenêtre de modification d'un favori (nom et dossier).
- Onglets en haut ou verticaux à gauche ; thème système, clair ou sombre ; 6 couleurs d'accent ; densité compacte ; taille de l'interface de 80 à 130 % ; bouton Accueil.
- Réglages en 12 sections avec recherche.
- Démarrage : reprendre la session, un nouvel onglet ou des pages précises. Zoom par défaut, dossier de téléchargement, Leirk comme navigateur par défaut.

**Recherche et import**
- 28 moteurs intégrés, plus ceux importés des navigateurs Chromium. On choisit un moteur ponctuellement avec un mot-clé ou un bang : `g chats`, `!yt lofi`, `w Rust`, `gpt …`.
- Import des signets et de l'historique depuis Brave, Chrome, Edge, Vivaldi, Opera, Opera GX, Chromium, Yandex, Firefox, Zen, LibreWolf, Floorp et Waterfox (tous les profils).
- Les mots de passe ne sont pas importés : les navigateurs Chromium récents les chiffrent avec une clé liée à l'application, illisible pour un autre programme. WebView2 propose en revanche d'enregistrer les nouveaux mots de passe.

## Raccourcis

| | |
|---|---|
| Ctrl+T / Ctrl+W / Ctrl+Maj+T | nouvel onglet / fermer / rouvrir |
| Ctrl+Maj+N | onglet privé |
| Ctrl+L, F6, Alt+D | barre d'adresse |
| F2, Ctrl+K | commandes rapides |
| Ctrl+Tab, Ctrl+1…9 | changer d'onglet |
| Ctrl+D | signet |
| Clic molette sur un onglet | fermer |
| Molette sur la barre d'onglets | changer d'onglet |

## Pas dans cette version

Rewards/BAT, Wallet, Leo, fenêtres Tor, synchronisation, extensions, gestionnaire de signets en arborescence (les signets se cherchent depuis F2), fenêtres multiples, popups liés à `window.opener` (ils s'ouvrent dans un nouvel onglet, ce qui peut gêner certaines connexions OAuth).

## Diagnostic

Les erreurs de l'interface et les commandes reçues sont écrites dans `%LOCALAPPDATA%\Leirk\leirk.log`.

## Code

- `src/tabs.rs` : modèle d'onglets, piles, mosaïque, politique de déchargement
- `src/omnibox.rs` : URL ou recherche, moteurs, mots-clés
- `src/shields.rs` : adblock-rust, exceptions par site
- `src/store.rs` : profil, session, historique SQLite
- `src/import.rs` : détection et import des autres navigateurs
- `src/main.rs` : fenêtre, WebView2, hooks natifs (filtre réseau, raccourcis, favicon, audio, capture)
- `ui/index.html` : toute l'interface, sans framework

Tests : `cargo test` ; preuves TDD dans `docs/tdd.md` ; revue de sécurité dans `docs/SECURITY-REVIEW.md`.
