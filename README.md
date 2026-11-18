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
| 12 sites ouverts un par un (4 vivants) | ~1 550–1 700 Mo, 13 processus | ~1 975 Mo, 25 processus |

Sur des sites lourds, l'essentiel restant est le processus GPU de WebView2. Les onglets vidéo qui jouent du son (YouTube, Twitch) ne sont volontairement jamais déchargés.

## Fonctionnalités

**Brave**
- Shields : moteur `adblock-rust` de Brave, avec EasyList, EasyPrivacy, uBlock filters et privacy, et Liste FR. Les listes sont rafraîchies tous les 4 jours. Blocage réseau et masquage cosmétique par site, compteur par page et total, désactivation par site.
- Onglets privés (InPrivate, Ctrl+Maj+N) : toute l'interface passe en indigo, rien n'est écrit sur le disque.
- Disposition de Brave : onglets au-dessus, Shields à droite de l'adresse, menu ☰.

**Vivaldi**
- Piles d'onglets : glisser un onglet au centre d'un autre, ou clic droit. La pile active affiche une seconde rangée.
- Mosaïque de 2 à 4 onglets : Ctrl+clic pour sélectionner, puis le bouton « Mosaïque », ou clic droit, ou la pile en mosaïque.
- Commandes rapides (F2 / Ctrl+K) : onglets, signets, historique et actions dans le même champ, recherche sans accents.

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

## Code

- `src/tabs.rs` : modèle d'onglets, piles, mosaïque, politique de déchargement
- `src/omnibox.rs` : URL ou recherche, moteurs, mots-clés
- `src/shields.rs` : adblock-rust, exceptions par site
- `src/store.rs` : profil, session, historique SQLite
- `src/import.rs` : détection et import des autres navigateurs
- `src/main.rs` : fenêtre, WebView2, hooks natifs (filtre réseau, raccourcis, favicon, audio, capture)
- `ui/index.html` : toute l'interface, sans framework

Tests : `cargo test` (26 tests) ; preuves TDD dans `docs/tdd.md`.
